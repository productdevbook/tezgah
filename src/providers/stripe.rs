//! Stripe, over Checkout.
//!
//! The shopper is sent to a `checkout.stripe.com` page and comes back with
//! nothing but a session id; the card never touches this process. The intent is
//! created with `capture_method=manual`, so authorising and capturing stay the
//! two separate things [`crate::payment`] treats them as.
//!
//! # Webhooks
//!
//! `Stripe-Signature` carries `t=<unix seconds>` and one or more `v1=<hex>`.
//! Each `v1` is `HMAC-SHA256("{t}.{body}")` under the endpoint's signing
//! secret. The comparison is constant time, and a timestamp further than
//! [`SIGNATURE_TOLERANCE`] from now is refused: a body and its signature stay
//! valid forever otherwise, and a replayed delivery is a replayed capture.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::id::PaymentSessionId;
use crate::money::Currency;
use crate::payment::{
    Authorization, AuthorizationStatus, AuthorizeRequest, CancelRequest, CaptureRequest,
    CaptureResult, PaymentProvider, RefundRequest, RefundResult, SessionRequest, SessionResponse,
    SessionStatus, WebhookEvent, WebhookKind,
};
use crate::providers::{
    DEFAULT_TIMEOUT, Exponents, from_minor, header, http_client, same_secret, to_minor,
};

pub const CODE: &str = "stripe";

/// How old a signed payload may be. Stripe's own libraries use five minutes.
pub const SIGNATURE_TOLERANCE: i64 = 300;

const API: &str = "https://api.stripe.com";

/// What this needs to talk to one Stripe account.
///
/// `Debug` prints neither key. A secret in a log is a secret that has leaked.
#[derive(Clone)]
pub struct StripeConfig {
    pub secret_key: String,
    /// The endpoint's signing secret, `whsec_…`. Only this verifies a webhook.
    pub webhook_secret: String,
    pub base_url: String,
    pub timeout: Duration,
    pub exponents: Exponents,
}

impl StripeConfig {
    pub fn new(secret_key: impl Into<String>, webhook_secret: impl Into<String>) -> StripeConfig {
        StripeConfig {
            secret_key: secret_key.into(),
            webhook_secret: webhook_secret.into(),
            base_url: API.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            exponents: Exponents::new(),
        }
    }

    pub fn at(mut self, base_url: impl Into<String>) -> StripeConfig {
        self.base_url = base_url.into();
        self
    }

    pub fn timing_out_after(mut self, timeout: Duration) -> StripeConfig {
        self.timeout = timeout;
        self
    }

    pub fn counting(mut self, exponents: Exponents) -> StripeConfig {
        self.exponents = exponents;
        self
    }
}

impl fmt::Debug for StripeConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StripeConfig")
            .field("base_url", &self.base_url)
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct Stripe {
    config: StripeConfig,
    http: reqwest::Client,
}

impl Stripe {
    pub fn new(config: StripeConfig) -> Result<Stripe> {
        if config.secret_key.trim().is_empty() {
            return Err(Error::invalid("stripe needs a secret key"));
        }
        let http = http_client(config.timeout, CODE)?;
        Ok(Stripe { config, http })
    }

    async fn post(
        &self,
        path: &str,
        form: &[(String, String)],
        idempotency: Option<&str>,
    ) -> Result<Value> {
        let mut request = self
            .http
            .post(format!("{}{path}", self.config.base_url))
            .bearer_auth(&self.config.secret_key)
            .timeout(self.config.timeout)
            .form(form);

        if let Some(key) = idempotency {
            request = request.header("Idempotency-Key", key);
        }

        let response = request
            .send()
            .await
            .map_err(|err| Error::provider(CODE, err.to_string()))?;

        answer(response).await
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .http
            .get(format!("{}{path}", self.config.base_url))
            .bearer_auth(&self.config.secret_key)
            .timeout(self.config.timeout)
            .send()
            .await
            .map_err(|err| Error::provider(CODE, err.to_string()))?;

        answer(response).await
    }
}

#[async_trait]
impl PaymentProvider for Stripe {
    fn code(&self) -> &'static str {
        CODE
    }

    async fn create_session(&self, req: SessionRequest) -> Result<SessionResponse> {
        let success = text(&req.context, "success_url")
            .ok_or_else(|| Error::invalid("a stripe session needs a success_url"))?;
        let cancel = text(&req.context, "cancel_url")
            .ok_or_else(|| Error::invalid("a stripe session needs a cancel_url"))?;
        let minor = to_minor(req.amount, &self.config.exponents)?;

        let mut form = vec![
            ("mode".to_owned(), "payment".to_owned()),
            ("success_url".to_owned(), success.to_owned()),
            ("cancel_url".to_owned(), cancel.to_owned()),
            ("client_reference_id".to_owned(), req.session_id.to_string()),
            (
                "payment_intent_data[capture_method]".to_owned(),
                "manual".to_owned(),
            ),
            (
                "line_items[0][price_data][currency]".to_owned(),
                req.amount.currency.as_str().to_ascii_lowercase(),
            ),
            (
                "line_items[0][price_data][unit_amount]".to_owned(),
                minor.to_string(),
            ),
            (
                "line_items[0][price_data][product_data][name]".to_owned(),
                text(&req.context, "description")
                    .unwrap_or("Order")
                    .to_owned(),
            ),
            ("line_items[0][quantity]".to_owned(), "1".to_owned()),
            (
                "metadata[session_id]".to_owned(),
                req.session_id.to_string(),
            ),
            (
                "metadata[collection_id]".to_owned(),
                req.collection_id.to_string(),
            ),
        ];

        if let Some(holder) = &req.account_holder {
            form.push(("customer".to_owned(), holder.clone()));
        } else if let Some(email) = text(&req.context, "email") {
            form.push(("customer_email".to_owned(), email.to_owned()));
        }

        let created = self
            .post(
                "/v1/checkout/sessions",
                &form,
                Some(&req.session_id.to_string()),
            )
            .await?;

        Ok(SessionResponse {
            data: json!({
                "checkout_session": created["id"],
                "url": created["url"],
                "payment_intent": created["payment_intent"],
            }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> Result<Authorization> {
        let checkout = text(&req.data, "checkout_session")
            .or_else(|| text(&req.context, "checkout_session"))
            .ok_or_else(|| Error::provider(CODE, "no checkout session to authorise"))?;

        let session = self
            .get(&format!(
                "/v1/checkout/sessions/{checkout}?expand[]=payment_intent"
            ))
            .await?;

        let intent = &session["payment_intent"];
        let intent_id = intent
            .as_str()
            .map(str::to_owned)
            .or_else(|| intent["id"].as_str().map(str::to_owned));
        let status = intent["status"].as_str().unwrap_or_default();

        let data = json!({
            "checkout_session": checkout,
            "payment_intent": intent_id,
            "intent_status": status,
        });

        let held = intent["amount_capturable"]
            .as_i64()
            .filter(|amount| *amount > 0)
            .or_else(|| intent["amount_received"].as_i64().filter(|a| *a > 0))
            .and_then(|minor| {
                Currency::parse(intent["currency"].as_str().unwrap_or_default())
                    .ok()
                    .map(|currency| from_minor(minor, currency, &self.config.exponents))
            });

        match status {
            "requires_capture" | "succeeded" => Ok(Authorization {
                status: AuthorizationStatus::Authorized,
                amount: held.or(Some(req.amount)),
                data,
                redirect: None,
                message: None,
            }),
            "requires_action"
            | "requires_confirmation"
            | "processing"
            | "requires_payment_method" => Ok(Authorization {
                status: AuthorizationStatus::RequiresMore,
                amount: None,
                data,
                redirect: session["url"].as_str().map(str::to_owned),
                message: None,
            }),
            _ => Ok(Authorization {
                status: AuthorizationStatus::Error,
                amount: None,
                data,
                redirect: None,
                message: Some(format!("stripe left the intent {status:?}")),
            }),
        }
    }

    async fn capture(&self, req: CaptureRequest) -> Result<CaptureResult> {
        let intent = text(&req.data, "payment_intent")
            .ok_or_else(|| Error::provider(CODE, "no payment intent to capture"))?;
        let minor = to_minor(req.amount, &self.config.exponents)?;

        let captured = self
            .post(
                &format!("/v1/payment_intents/{intent}/capture"),
                &[("amount_to_capture".to_owned(), minor.to_string())],
                Some(&idempotency_key("capture", intent, minor)),
            )
            .await?;

        let taken = captured["amount_received"]
            .as_i64()
            .map(|amount| from_minor(amount, req.amount.currency, &self.config.exponents))
            .unwrap_or(req.amount);

        Ok(CaptureResult {
            amount: taken,
            data: json!({ "payment_intent": intent, "status": captured["status"] }),
        })
    }

    async fn refund(&self, req: RefundRequest) -> Result<RefundResult> {
        let intent = text(&req.data, "payment_intent")
            .ok_or_else(|| Error::provider(CODE, "no payment intent to refund"))?;
        let minor = to_minor(req.amount, &self.config.exponents)?;

        let refunded = self
            .post(
                "/v1/refunds",
                &[
                    ("payment_intent".to_owned(), intent.to_owned()),
                    ("amount".to_owned(), minor.to_string()),
                ],
                Some(&idempotency_key("refund", intent, minor)),
            )
            .await?;

        let given = refunded["amount"]
            .as_i64()
            .map(|amount| from_minor(amount, req.amount.currency, &self.config.exponents))
            .unwrap_or(req.amount);

        Ok(RefundResult {
            amount: given,
            data: json!({ "refund": refunded["id"], "payment_intent": intent }),
        })
    }

    async fn cancel(&self, req: CancelRequest) -> Result<()> {
        let intent = text(&req.data, "payment_intent")
            .ok_or_else(|| Error::provider(CODE, "no payment intent to cancel"))?;

        self.post(
            &format!("/v1/payment_intents/{intent}/cancel"),
            &[],
            Some(&idempotency_key("cancel", intent, 0)),
        )
        .await?;

        Ok(())
    }

    fn parse_webhook(&self, headers: &[(String, String)], body: &[u8]) -> Result<WebhookEvent> {
        let signature = header(headers, "stripe-signature")
            .ok_or_else(|| Error::provider(CODE, "that delivery carried no signature"))?;

        verify_signature(
            &self.config.webhook_secret,
            signature,
            body,
            chrono::Utc::now().timestamp(),
            SIGNATURE_TOLERANCE,
        )?;

        let payload: Value = serde_json::from_slice(body)
            .map_err(|_| Error::provider(CODE, "a signed body that is not json"))?;

        read_event(&payload, &self.config.exponents)
    }
}

/// Checks one `Stripe-Signature` header against the body.
///
/// `now` and `tolerance` are arguments rather than read from the clock so the
/// replay window is testable without waiting five minutes.
pub fn verify_signature(
    secret: &str,
    header_value: &str,
    body: &[u8],
    now: i64,
    tolerance: i64,
) -> Result<()> {
    let mut timestamp: Option<i64> = None;
    let mut candidates: Vec<&str> = Vec::new();

    for part in header_value.split(',') {
        let mut halves = part.trim().splitn(2, '=');
        match (halves.next(), halves.next()) {
            (Some("t"), Some(value)) => timestamp = value.trim().parse::<i64>().ok(),
            (Some("v1"), Some(value)) => candidates.push(value.trim()),
            _ => {}
        }
    }

    let timestamp =
        timestamp.ok_or_else(|| Error::provider(CODE, "a signature with no timestamp"))?;
    if candidates.is_empty() {
        return Err(Error::provider(CODE, "a signature with no v1 scheme"));
    }
    if (now - timestamp).abs() > tolerance {
        return Err(Error::provider(CODE, "that signature is too old to trust"));
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| Error::provider(CODE, "an unusable webhook secret"))?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());

    let matched = candidates
        .iter()
        .any(|given| same_secret(given.as_bytes(), expected.as_bytes()));

    if matched {
        Ok(())
    } else {
        Err(Error::provider(CODE, "that signature is not ours"))
    }
}

/// Builds the header Stripe would have sent. For a host testing its own
/// webhook endpoint without a network — the only other caller is the
/// verification above, in reverse.
pub fn signature_header(secret: &str, timestamp: i64, body: &[u8]) -> Result<String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|_| Error::provider(CODE, "an unusable webhook secret"))?;
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    Ok(format!(
        "t={timestamp},v1={}",
        hex::encode(mac.finalize().into_bytes())
    ))
}

/// What a verified event means. An event tezgah does not model is
/// [`WebhookKind::Other`] rather than an error: refusing it would have Stripe
/// redeliver something nobody will ever act on, for days.
pub fn read_event(payload: &Value, exponents: &Exponents) -> Result<WebhookEvent> {
    let event_id = payload["id"]
        .as_str()
        .ok_or_else(|| Error::provider(CODE, "an event with no id"))?
        .to_owned();
    let event_type = payload["type"].as_str().unwrap_or_default().to_owned();
    let object = &payload["data"]["object"];

    let kind = match event_type.as_str() {
        "checkout.session.completed"
        | "checkout.session.async_payment_succeeded"
        | "payment_intent.amount_capturable_updated" => WebhookKind::Authorized,
        "payment_intent.succeeded" | "charge.captured" => WebhookKind::Captured,
        "charge.refunded" | "charge.refund.updated" | "refund.created" => WebhookKind::Refunded,
        "payment_intent.canceled" | "checkout.session.expired" => WebhookKind::Canceled,
        "payment_intent.payment_failed"
        | "charge.failed"
        | "checkout.session.async_payment_failed" => WebhookKind::Failed,
        _ => WebhookKind::Other,
    };

    let session_id = object["metadata"]["session_id"]
        .as_str()
        .or_else(|| object["client_reference_id"].as_str())
        .and_then(|raw| raw.parse::<PaymentSessionId>().ok());

    let amount = object["currency"]
        .as_str()
        .and_then(|code| Currency::parse(code).ok())
        .and_then(|currency| {
            object["amount_received"]
                .as_i64()
                .or_else(|| object["amount_capturable"].as_i64())
                .or_else(|| object["amount"].as_i64())
                .map(|minor| from_minor(minor, currency, exponents))
        });

    Ok(WebhookEvent {
        event_id,
        kind,
        event_type,
        session_id,
        amount,
        payload: payload.clone(),
    })
}

/// A key derived from what the call is, so a retry of the same capture is the
/// same key and Stripe answers with the first result instead of taking the
/// money twice.
fn idempotency_key(operation: &str, intent: &str, minor: i64) -> String {
    let mut digest = Sha256::new();
    digest.update(operation.as_bytes());
    digest.update(b":");
    digest.update(intent.as_bytes());
    digest.update(b":");
    digest.update(minor.to_string().as_bytes());
    format!("tezgah-{operation}-{}", hex::encode(digest.finalize()))
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

async fn answer(response: reqwest::Response) -> Result<Value> {
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|err| Error::provider(CODE, format!("an unreadable answer: {err}")))?;

    if status.is_success() {
        return Ok(body);
    }

    let message = body["error"]["message"]
        .as_str()
        .unwrap_or("refused without saying why");
    Err(Error::provider(CODE, format!("{status}: {message}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, timestamp: i64, body: &[u8]) -> String {
        signature_header(secret, timestamp, body).expect("a header")
    }

    #[test]
    fn a_signature_over_the_body_is_accepted() {
        let body = br#"{"id":"evt_1"}"#;
        let header = sign("whsec_test", 1_700_000_000, body);
        assert!(verify_signature("whsec_test", &header, body, 1_700_000_000, 300).is_ok());
    }

    #[test]
    fn a_body_changed_after_signing_is_refused() {
        let header = sign("whsec_test", 1_700_000_000, br#"{"id":"evt_1"}"#);
        assert!(
            verify_signature(
                "whsec_test",
                &header,
                br#"{"id":"evt_2"}"#,
                1_700_000_000,
                300
            )
            .is_err()
        );
    }
}
