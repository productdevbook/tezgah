//! iyzico, over the hosted checkout form.
//!
//! The shopper is sent to iyzico's own page and comes back with a token. **That
//! token is not evidence.** It arrives over the shopper's browser, so
//! [`PaymentProvider::authorize`] spends it by asking iyzico, with this
//! server's own credentials, what the payment actually is — and believes the
//! answer rather than the callback.
//!
//! # Signing a request
//!
//! `authorization: IYZWSv2 <base64>`, where the base64 covers
//! `apiKey:…&randomKey:…&signature:…` and the signature is
//! `HMAC-SHA256(randomKey + uriPath + body)` under the secret key, hex encoded.
//! `x-iyzi-rnd` carries the same random key so iyzico can rebuild it.
//!
//! # Webhooks
//!
//! `X-IYZ-SIGNATURE-V3` is `HMAC-SHA256(secretKey + eventType + paymentId +
//! conversationId + status)` under the secret key, hex encoded, compared in
//! constant time. iyzico gives a delivery no id of its own, so the event id is
//! built from the payment and what happened to it — which is what makes the
//! second delivery of one event look like the first to
//! [`record_webhook`](crate::payment::record_webhook).
//!
//! # Capture
//!
//! iyzico's checkout form takes the money at authorisation. There is no
//! separate capture call to make, so [`PaymentProvider::capture`] confirms what
//! was already taken instead of pretending to move it a second time.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::PaymentSessionId;
use crate::money::Money;
use crate::payment::{
    Authorization, AuthorizationStatus, AuthorizeRequest, CancelRequest, CaptureRequest,
    CaptureResult, Installment, PaymentProvider, RefundRequest, RefundResult, SessionRequest,
    SessionResponse, SessionStatus, SurchargeBearer, WebhookEvent, WebhookKind,
};
use crate::providers::{
    DEFAULT_TIMEOUT, Exponents, header, http_client, same_secret, to_decimal_string,
};

pub const CODE: &str = "iyzico";

const API: &str = "https://api.iyzipay.com";
const INITIALIZE: &str = "/payment/iyzipos/checkoutform/initialize/auth/ecom";
const RETRIEVE: &str = "/payment/iyzipos/checkoutform/auth/ecom/detail";
const REFUND: &str = "/payment/refund";
const CANCEL: &str = "/payment/cancel";

/// What this needs to talk to one iyzico merchant.
///
/// `Debug` prints neither key.
#[derive(Clone)]
pub struct IyzicoConfig {
    pub api_key: String,
    pub secret_key: String,
    pub base_url: String,
    pub timeout: Duration,
    pub locale: String,
    pub exponents: Exponents,
}

impl IyzicoConfig {
    pub fn new(api_key: impl Into<String>, secret_key: impl Into<String>) -> IyzicoConfig {
        IyzicoConfig {
            api_key: api_key.into(),
            secret_key: secret_key.into(),
            base_url: API.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            locale: "tr".to_owned(),
            exponents: Exponents::new(),
        }
    }

    pub fn at(mut self, base_url: impl Into<String>) -> IyzicoConfig {
        self.base_url = base_url.into();
        self
    }

    pub fn timing_out_after(mut self, timeout: Duration) -> IyzicoConfig {
        self.timeout = timeout;
        self
    }

    pub fn speaking(mut self, locale: impl Into<String>) -> IyzicoConfig {
        self.locale = locale.into();
        self
    }

    pub fn counting(mut self, exponents: Exponents) -> IyzicoConfig {
        self.exponents = exponents;
        self
    }
}

impl fmt::Debug for IyzicoConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IyzicoConfig")
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .field("locale", &self.locale)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Iyzico {
    config: IyzicoConfig,
    http: reqwest::Client,
}

impl Iyzico {
    pub fn new(config: IyzicoConfig) -> Result<Iyzico> {
        if config.api_key.trim().is_empty() || config.secret_key.trim().is_empty() {
            return Err(Error::invalid("iyzico needs an api key and a secret key"));
        }
        let http = http_client(config.timeout, CODE)?;
        Ok(Iyzico { config, http })
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let body = serde_json::to_string(body)
            .map_err(|_| Error::provider(CODE, "a request that will not serialise"))?;
        let random_key = random_key();
        let authorization = authorization_header(
            &self.config.api_key,
            &self.config.secret_key,
            &random_key,
            path,
            &body,
        )?;

        let response = self
            .http
            .post(format!("{}{path}", self.config.base_url))
            .header("authorization", authorization)
            .header("x-iyzi-rnd", &random_key)
            .header("content-type", "application/json")
            .timeout(self.config.timeout)
            .body(body)
            .send()
            .await
            .map_err(|err| Error::provider(CODE, err.to_string()))?;

        let answer: Value = response
            .json()
            .await
            .map_err(|err| Error::provider(CODE, format!("an unreadable answer: {err}")))?;

        if answer["status"].as_str() == Some("success") {
            Ok(answer)
        } else {
            Err(Error::provider(
                CODE,
                format!(
                    "{}: {}",
                    answer["errorCode"].as_str().unwrap_or("?"),
                    answer["errorMessage"]
                        .as_str()
                        .unwrap_or("refused without saying why")
                ),
            ))
        }
    }
}

#[async_trait]
impl PaymentProvider for Iyzico {
    fn code(&self) -> &'static str {
        CODE
    }

    async fn create_session(&self, req: SessionRequest) -> Result<SessionResponse> {
        let callback = text(&req.context, "callback_url")
            .ok_or_else(|| Error::invalid("an iyzico session needs a callback_url"))?;
        let price = to_decimal_string(req.amount, &self.config.exponents);

        let buyer = req
            .context
            .get("buyer")
            .cloned()
            .ok_or_else(|| Error::invalid("an iyzico session needs a buyer"))?;
        let address = req
            .context
            .get("address")
            .cloned()
            .ok_or_else(|| Error::invalid("an iyzico session needs an address"))?;
        let basket = req.context.get("basket_items").cloned().unwrap_or_else(|| {
            json!([{
                "id": req.collection_id.to_string(),
                "name": text(&req.context, "description").unwrap_or("Order"),
                "category1": "Order",
                "itemType": "VIRTUAL",
                "price": price,
            }])
        });

        let mut body = json!({
            "locale": self.config.locale,
            "conversationId": req.session_id.to_string(),
            "price": price,
            "paidPrice": price,
            "currency": req.amount.currency.as_str(),
            "basketId": req.collection_id.to_string(),
            "paymentGroup": "PRODUCT",
            "callbackUrl": callback,
            "buyer": buyer,
            "shippingAddress": address,
            "billingAddress": address,
            "basketItems": basket,
        });

        if let Some(holder) = &req.account_holder {
            body["cardUserKey"] = json!(holder);
        }

        let created = self.post(INITIALIZE, &body).await?;

        let token = created["token"]
            .as_str()
            .ok_or_else(|| Error::provider(CODE, "an initialised form with no token"))?;

        Ok(SessionResponse {
            data: json!({
                "token": token,
                "payment_page_url": created["paymentPageUrl"],
                "conversation_id": req.session_id.to_string(),
            }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> Result<Authorization> {
        let token = text(&req.context, "token")
            .or_else(|| text(&req.data, "token"))
            .ok_or_else(|| Error::provider(CODE, "no checkout form token to look up"))?;

        let detail = self
            .post(
                RETRIEVE,
                &json!({
                    "locale": self.config.locale,
                    "conversationId": req.session_id.to_string(),
                    "token": token,
                }),
            )
            .await?;

        let payment_status = detail["paymentStatus"].as_str().unwrap_or_default();
        let data = json!({
            "token": token,
            "payment_id": field(&detail, "paymentId"),
            "payment_transaction_id": first_transaction(&detail),
            "payment_status": payment_status,
        });

        if detail["conversationId"].as_str() != Some(req.session_id.to_string().as_str()) {
            return Err(Error::provider(
                CODE,
                "that token belongs to another session",
            ));
        }

        match payment_status {
            "SUCCESS" => {
                let paid = decimal(&detail, "paidPrice")
                    .map(|amount| Money::new(amount, req.amount.currency));
                Ok(Authorization {
                    status: AuthorizationStatus::Authorized,
                    amount: paid.or(Some(req.amount)),
                    data,
                    redirect: None,
                    message: None,
                    installment: plan(&detail, req.amount.currency),
                })
            }
            "INIT_THREEDS" | "CALLBACK_THREEDS" | "PENDING_CREDIT" | "" => Ok(Authorization {
                status: AuthorizationStatus::RequiresMore,
                amount: None,
                data,
                redirect: detail["paymentPageUrl"].as_str().map(str::to_owned),
                message: None,
                installment: None,
            }),
            _ => Ok(Authorization {
                status: AuthorizationStatus::Error,
                amount: None,
                data,
                redirect: None,
                message: Some(format!("iyzico left the payment {payment_status:?}")),
                installment: None,
            }),
        }
    }

    /// iyzico took the money when it authorised. This confirms that rather than
    /// asking for it again.
    async fn capture(&self, req: CaptureRequest) -> Result<CaptureResult> {
        let payment = text(&req.data, "payment_id")
            .ok_or_else(|| Error::provider(CODE, "no iyzico payment to capture"))?;

        Ok(CaptureResult {
            amount: req.amount,
            data: json!({ "payment_id": payment, "captured_at": "authorization" }),
        })
    }

    async fn refund(&self, req: RefundRequest) -> Result<RefundResult> {
        let transaction = text(&req.data, "payment_transaction_id")
            .ok_or_else(|| Error::provider(CODE, "no payment transaction to refund"))?;

        let refunded = self
            .post(
                REFUND,
                &json!({
                    "locale": self.config.locale,
                    "conversationId": req.payment_id.to_string(),
                    "paymentTransactionId": transaction,
                    "price": to_decimal_string(req.amount, &self.config.exponents),
                    "currency": req.amount.currency.as_str(),
                }),
            )
            .await?;

        let given = refunded["price"]
            .as_str()
            .and_then(|raw| Decimal::from_str(raw).ok())
            .map(|amount| Money::new(amount, req.amount.currency))
            .unwrap_or(req.amount);

        Ok(RefundResult {
            amount: given,
            data: json!({
                "payment_transaction_id": transaction,
                "payment_id": refunded["paymentId"],
            }),
        })
    }

    async fn cancel(&self, req: CancelRequest) -> Result<()> {
        let payment = text(&req.data, "payment_id")
            .ok_or_else(|| Error::provider(CODE, "no iyzico payment to cancel"))?;

        self.post(
            CANCEL,
            &json!({
                "locale": self.config.locale,
                "conversationId": payment,
                "paymentId": payment,
            }),
        )
        .await?;

        Ok(())
    }

    fn parse_webhook(&self, headers: &[(String, String)], body: &[u8]) -> Result<WebhookEvent> {
        let signature = header(headers, "x-iyz-signature-v3")
            .ok_or_else(|| Error::provider(CODE, "that delivery carried no signature"))?;

        let payload: Value = serde_json::from_slice(body)
            .map_err(|_| Error::provider(CODE, "a delivery that is not json"))?;

        verify_webhook_signature(&self.config.secret_key, signature, &payload)?;

        read_event(&payload)
    }
}

/// Builds the `authorization` header for one request.
///
/// The body must be the exact bytes sent: iyzico signs the string, not a
/// re-serialisation of it.
pub fn authorization_header(
    api_key: &str,
    secret_key: &str,
    random_key: &str,
    uri_path: &str,
    body: &str,
) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
        .map_err(|_| Error::provider(CODE, "an unusable secret key"))?;
    mac.update(random_key.as_bytes());
    mac.update(uri_path.as_bytes());
    mac.update(body.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let params = format!("apiKey:{api_key}&randomKey:{random_key}&signature:{signature}");
    Ok(format!("IYZWSv2 {}", BASE64.encode(params)))
}

/// Checks `X-IYZ-SIGNATURE-V3` against the fields it covers.
pub fn verify_webhook_signature(secret_key: &str, given: &str, payload: &Value) -> Result<()> {
    let expected = webhook_signature(secret_key, payload)?;
    if same_secret(given.trim().as_bytes(), expected.as_bytes()) {
        Ok(())
    } else {
        Err(Error::provider(CODE, "that signature is not ours"))
    }
}

/// `HMAC-SHA256(secretKey + eventType + paymentId + conversationId + status)`.
pub fn webhook_signature(secret_key: &str, payload: &Value) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret_key.as_bytes())
        .map_err(|_| Error::provider(CODE, "an unusable secret key"))?;
    mac.update(secret_key.as_bytes());
    mac.update(field(payload, "iyziEventType").as_bytes());
    mac.update(field(payload, "paymentId").as_bytes());
    mac.update(field(payload, "paymentConversationId").as_bytes());
    mac.update(field(payload, "status").as_bytes());
    Ok(hex::encode(mac.finalize().into_bytes()))
}

/// What a verified notification means.
///
/// iyzico gives a delivery no id, so one is built from the payment and what
/// happened to it: the same notification twice is the same id twice, which is
/// what makes the second one a no-op. An event type tezgah does not model is
/// [`WebhookKind::Other`] — acknowledged and ignored, rather than refused into
/// a redelivery loop.
pub fn read_event(payload: &Value) -> Result<WebhookEvent> {
    let event_type = field(payload, "iyziEventType");
    let payment_id = field(payload, "paymentId");
    let status = field(payload, "status");

    if payment_id.is_empty() {
        return Err(Error::provider(CODE, "a notification with no payment"));
    }

    let kind = match (event_type.as_str(), status.as_str()) {
        (_, "FAILURE") => WebhookKind::Failed,
        ("CHECKOUTFORM_AUTH" | "API_AUTH" | "THREE_DS_AUTH" | "BKM_AUTH", _) => {
            WebhookKind::Authorized
        }
        ("BALANCE_UPDATE", _) => WebhookKind::Captured,
        ("REFUND" | "REFUND_V2", _) => WebhookKind::Refunded,
        ("CANCEL", _) => WebhookKind::Canceled,
        _ => WebhookKind::Other,
    };

    let session_id = payload["paymentConversationId"]
        .as_str()
        .and_then(|raw| raw.parse::<PaymentSessionId>().ok());

    Ok(WebhookEvent {
        event_id: format!("{event_type}:{payment_id}:{status}"),
        kind,
        event_type,
        session_id,
        amount: None,
        payload: payload.clone(),
    })
}

fn field(payload: &Value, key: &str) -> String {
    match &payload[key] {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

/// iyzico writes an amount as a string in some answers and as a number in
/// others.
fn decimal(value: &Value, key: &str) -> Option<Decimal> {
    value[key]
        .as_str()
        .and_then(|raw| Decimal::from_str(raw).ok())
        .or_else(|| value[key].as_f64().and_then(Decimal::from_f64_retain))
}

/// `price` is what the basket came to and `paidPrice` what the card is
/// charged. The difference is the vade farkı the shopper agreed to for
/// splitting it, and it is the reason an instalment sale authorises more than
/// the order total.
fn plan(detail: &Value, currency: crate::money::Currency) -> Option<Installment> {
    let count = detail["installment"].as_i64().unwrap_or(1) as i32;
    if count < 2 {
        return None;
    }
    let basket = decimal(detail, "price")?;
    let charged = decimal(detail, "paidPrice")?;
    Some(Installment {
        count,
        surcharge: Money::new((charged - basket).max(Decimal::ZERO), currency),
        bearer: SurchargeBearer::Customer,
        campaign: text(detail, "binNumber").map(str::to_owned),
    })
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// iyzico writes ids as numbers in some answers and as strings in others, so
/// what is stored is always the string.
fn first_transaction(detail: &Value) -> String {
    field(&detail["itemTransactions"][0], "paymentTransactionId")
}

fn random_key() -> String {
    format!(
        "{}{}",
        chrono::Utc::now().timestamp_millis(),
        Uuid::now_v7().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_authorization_header_is_base64_of_the_three_parameters() {
        let header =
            authorization_header("api", "secret", "rnd", "/payment/auth", "{}").expect("a header");
        let decoded = BASE64
            .decode(header.trim_start_matches("IYZWSv2 "))
            .expect("base64");
        let params = String::from_utf8(decoded).expect("utf8");
        assert!(params.starts_with("apiKey:api&randomKey:rnd&signature:"));
    }
}
