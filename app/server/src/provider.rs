//! `PaymentProvider` over `dyn kasapay_core::Provider`, built the same way
//! `examples/shop/provider.rs` builds one — see that file's doc comment for
//! why `capture`, `refund`, `cancel` and `lookup` are thin calls onto
//! `tezgah::providers::kasapay`'s mapping while `create_session` and
//! `authorize` are this file's own.
//!
//! # `DemoBank` is not a payment provider
//!
//! tezgah does not ship one — `CLAUDE.md` is explicit that a provider is
//! kasapay's to write, not tezgah's, and no adapter crate for a real bank or
//! gateway lives in this public repository. `DemoBank` authorises every
//! charge it is asked for and remembers nothing, so that
//! `POST /store/carts/{id}/complete` has something to run against instead of
//! being unbound. Taking real money means building against a real
//! `kasapay_core::Provider` — an adapter crate from the kasapay project — and
//! passing that to `KasapayProvider::new` in `main.rs` in place of
//! `DemoBank`. Nothing here should be mistaken for that crate arriving.
//!
//! `main.rs` never wires this in on its own: `config::Config::demo_bank_enabled`
//! has to be `true` — `TEZGAH_DEMO_BANK` set to exactly
//! `i-understand-this-takes-no-money` — or checkout stays unbound, the same
//! way it stays unbound with no stock location. A phrase that has to be typed
//! out cannot be set by accident, and the default is closed for the same
//! reason `ADMIN_TOKEN`'s default is: a self-hosted install that never
//! touched this variable takes no money it did not mean to.

use std::sync::Arc;

use async_trait::async_trait;
use tezgah::payment::{
    Authorization, AuthorizeRequest, CancelRequest, CaptureRequest, CaptureResult, LookupProvider,
    PaymentProvider, RefundRequest, RefundResult, SessionRequest, SessionResponse, SessionStatus,
    WebhookEvent,
};
use tezgah::providers::kasapay as mapping;

/// `exponent` fixes this shop to one currency's decimal places —
/// `config::Config::currency_exponent`, read from `TEZGAH_CURRENCY_EXPONENT`.
/// A host selling in more than one currency needs a mapping that reads
/// `store::exponent` per amount instead; that is not what a single
/// `KasapayProvider` can do, so it is not what this binary offers.
#[derive(Debug)]
pub struct KasapayProvider {
    provider: Arc<dyn kasapay_core::Provider>,
    exponent: u32,
}

impl KasapayProvider {
    pub fn new(provider: Arc<dyn kasapay_core::Provider>, exponent: u32) -> Self {
        KasapayProvider { provider, exponent }
    }

    fn kasapay_payment_id(data: &serde_json::Value) -> tezgah::Result<String> {
        data.get("kasapay_payment_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| {
                tezgah::Error::invalid(
                    "no kasapay payment id was recorded for that payment — it was never \
                     authorized through this provider",
                )
            })
    }
}

fn map_provider_error(err: kasapay_core::Error) -> tezgah::Error {
    tezgah::Error::provider(err.provider().as_str(), err.to_string())
}

#[async_trait]
impl PaymentProvider for KasapayProvider {
    fn code(&self) -> &'static str {
        "kasapay"
    }

    async fn create_session(&self, req: SessionRequest) -> tezgah::Result<SessionResponse> {
        Ok(SessionResponse {
            data: serde_json::json!({ "session_id": req.session_id.to_string() }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> tezgah::Result<Authorization> {
        let order = kasapay_core::OrderRef::new(req.session_id.to_string());
        let amount = mapping::to_kasapay_money(req.amount, self.exponent)?;
        let request = kasapay_core::ChargeRequest::builder(order, amount)
            .build()
            .map_err(|err| tezgah::Error::invalid(err.to_string()))?;

        let charge = self
            .provider
            .charge(&request)
            .await
            .map_err(map_provider_error)?;

        let status = mapping::map_status(charge.status)?;
        let redirect = match &charge.next_action {
            Some(kasapay_core::NextAction::Redirect { url, .. }) => Some(url.to_string()),
            _ => None,
        };
        let raw = charge.raw.json().unwrap_or_else(|| serde_json::json!({}));

        Ok(Authorization {
            status,
            amount: Some(mapping::from_kasapay_money(charge.amount, self.exponent)?),
            data: serde_json::json!({
                "kasapay_payment_id": charge.id.as_ref().map(kasapay_core::PaymentId::as_str),
                "raw": raw,
            }),
            redirect,
            message: None,
            installment: None,
        })
    }

    async fn capture(&self, req: CaptureRequest) -> tezgah::Result<CaptureResult> {
        let id = Self::kasapay_payment_id(&req.data)?;
        mapping::capture(self.provider.as_ref(), &id, req.amount, self.exponent, None).await
    }

    async fn refund(&self, req: RefundRequest) -> tezgah::Result<RefundResult> {
        let id = Self::kasapay_payment_id(&req.data)?;
        mapping::refund(self.provider.as_ref(), &id, req.amount, self.exponent, None).await
    }

    async fn cancel(&self, req: CancelRequest) -> tezgah::Result<()> {
        let Ok(id) = Self::kasapay_payment_id(&req.data) else {
            return Ok(());
        };
        mapping::cancel(self.provider.as_ref(), &id).await
    }

    fn parse_webhook(
        &self,
        _headers: &[(String, String)],
        _body: &[u8],
    ) -> tezgah::Result<WebhookEvent> {
        Err(tezgah::Error::invalid(
            "this server binds no webhook route yet — see README.md's route table",
        ))
    }

    fn as_lookup(&self) -> Option<&dyn LookupProvider> {
        Some(self)
    }
}

// There is no `impl RecurringProvider for KasapayProvider` here, and its
// absence is the answer rather than an omission. Charging an instrument a
// shopper left on file needs the request to name that instrument, and
// kasapay 0.0.5 — the version this crate pins, and the newest published —
// has no field for one: `ChargeRequest` carries a `customer` and nothing to
// say which of their saved cards to take.
//
// It is not missing from kasapay, only from every version of it: the commit
// that added `ChargeRequest::instrument` landed eleven hours after v0.0.5 was
// tagged, so it is on main and in no release. productdevbook/kasapay#225 asks
// for one.
//
// Naming the customer alone and calling it a stored charge is precisely the
// "accept a field and drop it" that kasapay's own doc refuses, so this does
// neither. Until there is a release, `host::Dispatcher` records exactly this
// as a dunning retry's reason instead of marking the job done.
#[async_trait]
impl LookupProvider for KasapayProvider {
    async fn lookup(
        &self,
        session_id: tezgah::id::PaymentSessionId,
    ) -> tezgah::Result<Option<Authorization>> {
        mapping::lookup(
            self.provider.as_ref(),
            &session_id.to_string(),
            self.exponent,
        )
        .await
    }
}

fn unsupported(id: kasapay_core::ProviderId) -> kasapay_core::Error {
    kasapay_core::Error::new(
        kasapay_core::ErrorKind::Unsupported,
        id,
        "not exercised by this demo",
    )
}

/// Authorises every charge immediately and remembers nothing. `main.rs` only
/// constructs this behind `TEZGAH_DEMO_BANK` — see this file's own doc
/// comment for what standing it up as a demo does and does not mean.
#[derive(Debug, Default)]
pub struct DemoBank;

#[kasapay_core::async_trait]
impl kasapay_core::Provider for DemoBank {
    fn id(&self) -> kasapay_core::ProviderId {
        kasapay_core::ProviderId::new("demo-bank")
    }

    async fn charge(
        &self,
        request: &kasapay_core::ChargeRequest,
    ) -> Result<kasapay_core::Charge, kasapay_core::Error> {
        Ok(kasapay_core::Charge {
            id: Some(kasapay_core::PaymentId::issued(format!(
                "demo_{}",
                request.order
            ))),
            order: Some(kasapay_core::OrderRef::new(
                request.order.as_str().to_owned(),
            )),
            amount: request.amount,
            order_amount: None,
            status: kasapay_core::Status::Authorized,
            next_action: None,
            provider: self.id(),
            // The payer is echoed rather than dropped. kasapay's own doc is
            // explicit that accepting a field and ignoring it reads as a
            // guarantee where there is none, and a demo is where that habit
            // would start.
            raw: kasapay_core::Raw::from_json(&serde_json::json!({
                "demo": true,
                "customer": request.customer.as_deref(),
            })),
        })
    }

    async fn resume(
        &self,
        _continuation: &str,
    ) -> Result<kasapay_core::Charge, kasapay_core::Error> {
        Err(unsupported(self.id()))
    }

    async fn charge_status(
        &self,
        _id: &kasapay_core::PaymentId,
    ) -> Result<kasapay_core::Charge, kasapay_core::Error> {
        Err(unsupported(self.id()))
    }

    async fn capture(
        &self,
        id: &kasapay_core::PaymentId,
        amount: Option<kasapay_core::Money>,
        _idempotency: Option<&kasapay_core::IdempotencyKey>,
    ) -> Result<kasapay_core::Charge, kasapay_core::Error> {
        Ok(kasapay_core::Charge {
            id: Some(kasapay_core::PaymentId::issued(id.as_str().to_owned())),
            order: None,
            amount: amount.unwrap_or_else(|| {
                kasapay_core::Money::from_minor_units(0, kasapay_core::Currency::Try)
            }),
            order_amount: None,
            status: kasapay_core::Status::Captured,
            next_action: None,
            provider: self.id(),
            raw: kasapay_core::Raw::from_json(&serde_json::json!({ "demo": "captured" })),
        })
    }

    async fn cancel(
        &self,
        id: &kasapay_core::PaymentId,
    ) -> Result<kasapay_core::Charge, kasapay_core::Error> {
        Ok(kasapay_core::Charge {
            id: Some(kasapay_core::PaymentId::issued(id.as_str().to_owned())),
            order: None,
            amount: kasapay_core::Money::from_minor_units(0, kasapay_core::Currency::Try),
            order_amount: None,
            status: kasapay_core::Status::Canceled,
            next_action: None,
            provider: self.id(),
            raw: kasapay_core::Raw::from_json(&serde_json::json!({ "demo": "canceled" })),
        })
    }

    async fn refund(
        &self,
        request: &kasapay_core::RefundRequest,
    ) -> Result<kasapay_core::Refund, kasapay_core::Error> {
        Ok(kasapay_core::Refund {
            id: None,
            payment: request.payment.clone(),
            amount: request.amount.unwrap_or_else(|| {
                kasapay_core::Money::from_minor_units(0, kasapay_core::Currency::Try)
            }),
            status: kasapay_core::RefundStatus::Succeeded,
            next_action: None,
            provider: self.id(),
            raw: kasapay_core::Raw::from_json(&serde_json::json!({ "demo": "refunded" })),
        })
    }

    async fn lookup(
        &self,
        _order: &kasapay_core::OrderRef,
    ) -> Result<Option<kasapay_core::Charge>, kasapay_core::Error> {
        Ok(None)
    }

    async fn instruments(
        &self,
        _customer: &str,
    ) -> Result<Vec<kasapay_core::Instrument>, kasapay_core::Error> {
        Err(unsupported(self.id()))
    }

    fn capabilities(&self) -> kasapay_core::Capabilities {
        kasapay_core::Capabilities {
            separate_capture: true,
            partial_capture: true,
            partial_refund: true,
            repeated_refund: true,
            lookup_by_order: true,
            resume_by_continuation: false,
            saved_instruments: false,
        }
    }
}
