//! `PaymentProvider` over `dyn kasapay_core::Provider`, using the mapping in
//! `src/providers/kasapay.rs` everywhere that mapping reaches, and — issue
//! #199's second question — the first thing outside that module's own tests
//! to lean on it.
//!
//! `capture`, `refund`, `cancel` and `lookup` call the `pub` functions there
//! directly: tezgah#53's design note found those four collapse onto
//! `kasapay_core::Provider` close to 1:1, and doing exactly that below is
//! what tests it against a real `PaymentProvider` implementation rather than
//! against `src/providers/kasapay.rs`'s own unit tests, which only ever call
//! the functions, never a trait built from them.
//!
//! `create_session` and `authorize` are this file's own. The same design
//! note found those two do *not* collapse onto `kasapay_core::Provider`
//! uniformly across every provider — that is exactly why
//! `src/providers/kasapay.rs` ships no mapping for either — so `authorize`
//! below is where this example has to open a `kasapay_core::ChargeRequest`
//! and read a `Charge` back itself. Reading it back needs the same six-way
//! `Status` match `capture`/`cancel`/`refund`/`lookup` already reach through
//! the trait, and since tezgah#205 that match is `mapping::map_status` —
//! `pub` now, so this is the first host to call it rather than the first to
//! copy it.

use std::sync::Arc;

// Two different `async_trait` macros, on purpose: `kasapay_core::Provider` is
// defined with its own re-exported copy, and its doc says why —
// `kasapay_core::async_trait` is "re-exported because the version has to
// match the one this trait was defined with; matching it by hand is a
// footgun". `PaymentProvider`/`LookupProvider` are tezgah's own, defined
// against the plain `async-trait` crate tezgah depends on directly.
use async_trait::async_trait;
use tezgah::payment::{
    Authorization, AuthorizeRequest, CancelRequest, CaptureRequest, CaptureResult, LookupProvider,
    PaymentProvider, RefundRequest, RefundResult, SessionRequest, SessionResponse, SessionStatus,
    WebhookEvent,
};
use tezgah::providers::kasapay as mapping;

/// Wraps any `kasapay_core::Provider` — a real adapter crate's, or, as here
/// (`DemoBank`, below), a fake standing in for one so this example needs no
/// network and no account with anybody.
///
/// `exponent` is the shop's single currency's — the same number
/// `store::exponent` would answer for it. A host selling in more than one
/// currency would look this up per amount instead of fixing it once at
/// construction; nothing about `PaymentProvider`'s methods hands them a `Tx`
/// to read `store::exponent` with, which is the trade this example makes by
/// having exactly one currency.
#[derive(Debug)]
pub struct KasapayProvider {
    provider: Arc<dyn kasapay_core::Provider>,
    exponent: u32,
}

impl KasapayProvider {
    pub fn new(provider: Arc<dyn kasapay_core::Provider>, exponent: u32) -> Self {
        KasapayProvider { provider, exponent }
    }

    /// What `authorize` below stored so a later `capture`, `refund` or
    /// `cancel` can name the same payment at kasapay — `payment.data` is a
    /// host's own to fill, per `crate::payment::CaptureRequest` and its
    /// siblings, and this is what this provider expects to find there.
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

    /// `kasapay_core::Provider` has no separate "open a session" call — one
    /// `charge` starts, and for most providers finishes, a payment — so
    /// there is nothing to send yet. `authorize` below is where the
    /// provider is actually asked.
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
            // A session that never became a payment at the provider has
            // nothing there to release.
            return Ok(());
        };
        mapping::cancel(self.provider.as_ref(), &id).await
    }

    /// `kasapay_core::Provider` carries no webhook call at all — verifying a
    /// signature is each adapter's own, over its own headers — so there is
    /// no mapping in `src/providers/kasapay.rs` this could reuse even in
    /// principle. This example takes none.
    fn parse_webhook(
        &self,
        _headers: &[(String, String)],
        _body: &[u8],
    ) -> tezgah::Result<WebhookEvent> {
        Err(tezgah::Error::invalid(
            "this example provider takes no webhooks",
        ))
    }

    fn as_lookup(&self) -> Option<&dyn LookupProvider> {
        Some(self)
    }
}

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
        "not exercised by this example",
    )
}

/// A `kasapay_core::Provider` that authorises immediately and remembers
/// nothing — standing in for a real adapter crate the same way tezgah's own
/// tests do (`src/providers/kasapay.rs`'s `FakeKasapay`, which is
/// `#[cfg(test)]` and unreachable from here). tezgah#53's own doc says a
/// real provider crate is never a dependency of the library; this is that
/// same rule seen from an example's side of it.
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
            raw: kasapay_core::Raw::from_json(&serde_json::json!({ "demo": true })),
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
