//! Vocabulary borrowed from [kasapay](https://github.com/productdevbook/kasapay) — tezgah#53.
//!
//! Not a live adapter, and nothing here is called from [`crate::checkout`] or
//! [`crate::subscription`] yet. `src/providers/stripe.rs` and `iyzico.rs`
//! remain the production path: #53's design note found that
//! [`PaymentProvider::create_session`]/`authorize` do not collapse onto
//! [`kasapay_core::Provider::charge`] uniformly across both shipped
//! providers, so no wrapper implements [`PaymentProvider`] over
//! `kasapay_core::Provider` here. `capture`, `cancel`, `refund` and `lookup`
//! do map close to 1:1 — that mapping is what this module is, kept ready for
//! the adapter that will call it — plus the [`Money`] conversion boundary it
//! depends on, proven lossless before anything is built on top of it.
//!
//! Deleting `src/providers/stripe.rs` waits on
//! [kasapay#149](https://github.com/productdevbook/kasapay/issues/149):
//! `kasapay_core::Currency` is nine variants, closed on purpose for iyzico's
//! short settlement list, but shared by every adapter including Stripe's,
//! which settles in 135+. Closing tezgah's own escape hatch before that lands
//! somewhere else to go is the one thing this module does not do.
//!
//! [`PaymentProvider`]: crate::payment::PaymentProvider
//! [`PaymentProvider::create_session`]: crate::payment::PaymentProvider::create_session

use std::str::FromStr;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::error::{Error, Result};
use crate::money::{Currency, Money};
use crate::payment::{Authorization, AuthorizationStatus, CaptureResult, RefundResult};

/// Converts a tezgah amount into kasapay's minor-unit [`kasapay_core::Money`].
///
/// Errors rather than rounding or dropping: kasapay's `Currency` is nine
/// variants, closed on purpose, and a shop selling outside them cannot be
/// represented in a `ChargeRequest` at all — see this module's own doc and
/// kasapay#149. Silently falling back to a nearby currency would be a shop
/// charged in the wrong money; that is worse than refusing the call.
pub fn to_kasapay_money(money: Money) -> Result<kasapay_core::Money> {
    let currency = kasapay_core::Currency::from_str(money.currency.as_str()).map_err(|_| {
        Error::invalid(format!(
            "kasapay does not know currency {} (kasapay#149)",
            money.currency
        ))
    })?;
    let exponent = currency.exponent();
    let scale = Decimal::from(10u64.pow(exponent));
    let minor = (money.amount.round_dp(exponent) * scale)
        .round()
        .to_i64()
        .ok_or_else(|| Error::invalid("that amount does not fit kasapay's minor units"))?;
    Ok(kasapay_core::Money::from_minor_units(minor, currency))
}

/// The inverse of [`to_kasapay_money`], for reading an amount back out of
/// kasapay's answer.
///
/// `kasapay_core::Currency::code()` always writes three ASCII letters, so
/// [`Currency::parse`] cannot fail on it in practice — but this still reports
/// rather than assumes, because a library does not panic on a fact about
/// another crate that only holds today.
pub fn from_kasapay_money(money: kasapay_core::Money) -> Result<Money> {
    let currency = Currency::parse(money.currency().code())
        .map_err(|_| Error::bug("kasapay named a currency tezgah could not parse"))?;
    let exponent = money.currency().exponent();
    let scale = Decimal::from(10u64.pow(exponent));
    Ok(Money::new(
        Decimal::from(money.minor_units()) / scale,
        currency,
    ))
}

fn from_kasapay_error(err: kasapay_core::Error) -> Error {
    Error::provider(err.provider().as_str(), err.to_string())
}

/// `capture` for a provider whose kasapay adapter answers close to 1:1: takes
/// the amount already authorised, off the payment kasapay itself named.
///
/// `payment_id` is the provider's own identifier — what
/// [`kasapay_core::PaymentId::issued`] wraps — never tezgah's own
/// [`crate::id::PaymentId`], which kasapay has never heard of.
pub async fn capture(
    provider: &dyn kasapay_core::Provider,
    payment_id: &str,
    amount: Money,
    idempotency_key: Option<&str>,
) -> Result<CaptureResult> {
    let id = kasapay_core::PaymentId::issued(payment_id);
    let minor = to_kasapay_money(amount)?;
    let key = idempotency_key.map(kasapay_core::IdempotencyKey::new);
    let charge = provider
        .capture(&id, Some(minor), key.as_ref())
        .await
        .map_err(from_kasapay_error)?;
    Ok(CaptureResult {
        amount: from_kasapay_money(charge.amount)?,
        data: charge.raw.json().unwrap_or_else(|| serde_json::json!({})),
    })
}

/// `cancel` for a provider whose kasapay adapter answers close to 1:1:
/// releases an authorisation that will never be taken.
pub async fn cancel(provider: &dyn kasapay_core::Provider, payment_id: &str) -> Result<()> {
    let id = kasapay_core::PaymentId::issued(payment_id);
    provider.cancel(&id).await.map_err(from_kasapay_error)?;
    Ok(())
}

/// `refund` for a provider whose kasapay adapter answers close to 1:1: gives
/// money back off a payment kasapay already captured.
pub async fn refund(
    provider: &dyn kasapay_core::Provider,
    payment_id: &str,
    amount: Money,
    idempotency_key: Option<&str>,
) -> Result<RefundResult> {
    let id = kasapay_core::PaymentId::issued(payment_id);
    let minor = to_kasapay_money(amount)?;
    let mut builder = kasapay_core::RefundRequest::builder(id).amount(minor);
    if let Some(key) = idempotency_key {
        builder = builder.idempotency_key(kasapay_core::IdempotencyKey::new(key));
    }
    let request = builder
        .build()
        .map_err(|err| Error::invalid(err.to_string()))?;
    let refund = provider
        .refund(&request)
        .await
        .map_err(from_kasapay_error)?;
    Ok(RefundResult {
        amount: from_kasapay_money(refund.amount)?,
        data: refund.raw.json().unwrap_or_else(|| serde_json::json!({})),
    })
}

/// `lookup` for a provider whose kasapay adapter answers
/// [`kasapay_core::Capabilities::lookup_by_order`]: what became of a charge
/// whose HTTP response never arrived, asked by the caller's own reference.
///
/// The three-way answer is kasapay's own and is kept apart exactly as
/// [`crate::payment::LookupProvider`] already requires: `Ok(None)` is nothing
/// taken and safe to retry, `Ok(Some(_))` is what happened, and `Err(_)` is
/// the question going unanswered — never collapsed into the first, which is
/// how a shopper is charged twice.
pub async fn lookup(
    provider: &dyn kasapay_core::Provider,
    order_ref: &str,
) -> Result<Option<Authorization>> {
    let order = kasapay_core::OrderRef::new(order_ref.to_owned());
    match provider.lookup(&order).await {
        Ok(None) => Ok(None),
        Ok(Some(charge)) => Ok(Some(to_authorization(charge)?)),
        Err(err) => Err(from_kasapay_error(err)),
    }
}

/// kasapay's [`kasapay_core::Status`] is six variants and `#[non_exhaustive]`;
/// tezgah's [`AuthorizationStatus`] is three. `Authorized` and `Captured` both
/// become `Authorized` here — iyzico's own checkout form already conflates
/// the two in tezgah's existing adapter (`src/providers/iyzico.rs`'s
/// `capture` confirms money already taken rather than moving it again) — and
/// `Pending`/`RequiresAction` both become `RequiresMore`, which is not quite
/// honest: `Pending` means "still processing", not "the shopper has
/// something to do". tezgah's enum has no third bucket for it. Not resolved
/// here; noted so the day `RequiresMore` gains a poll-again cousin, this is
/// where to look.
fn map_status(status: kasapay_core::Status) -> AuthorizationStatus {
    use kasapay_core::Status;
    match status {
        Status::Authorized | Status::Captured => AuthorizationStatus::Authorized,
        Status::Pending | Status::RequiresAction => AuthorizationStatus::RequiresMore,
        Status::Failed | Status::Canceled => AuthorizationStatus::Error,
        _ => AuthorizationStatus::RequiresMore,
    }
}

fn to_authorization(charge: kasapay_core::Charge) -> Result<Authorization> {
    let redirect = match &charge.next_action {
        Some(kasapay_core::NextAction::Redirect { url, .. }) => Some(url.to_string()),
        _ => None,
    };
    Ok(Authorization {
        status: map_status(charge.status),
        amount: Some(from_kasapay_money(charge.amount)?),
        data: charge.raw.json().unwrap_or_else(|| serde_json::json!({})),
        redirect,
        message: None,
        installment: None,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use kasapay_core::{
        Capabilities, Charge, ChargeRequest, Currency as KCurrency, Error as KError,
        ErrorKind as KErrorKind, Instrument, Money as KMoney, OrderRef, PaymentId, ProviderId, Raw,
        Refund, RefundRequest, RefundStatus, Status, async_trait,
    };
    use rust_decimal_macros::dec;

    use super::*;

    fn try_(code: &str) -> Currency {
        Currency::parse(code).expect("a currency code")
    }

    // -----------------------------------------------------------------
    // Money — lossless in both directions, and an explicit error rather
    // than a silent round or drop for a currency kasapay does not know.
    // -----------------------------------------------------------------

    #[test]
    fn round_trips_a_two_decimal_currency() {
        let money = Money::new(dec!(10.50), try_("TRY"));
        let kasapay = to_kasapay_money(money).expect("TRY is one of the nine");
        assert_eq!(kasapay.minor_units(), 1050);
        assert_eq!(from_kasapay_money(kasapay).expect("round trips"), money);
    }

    #[test]
    fn round_trips_usd() {
        let money = Money::new(dec!(7.05), try_("USD"));
        let kasapay = to_kasapay_money(money).expect("USD is one of the nine");
        assert_eq!(kasapay.minor_units(), 705);
        assert_eq!(from_kasapay_money(kasapay).expect("round trips"), money);
    }

    #[test]
    fn round_trips_jpy_with_no_decimal_places() {
        let money = Money::new(dec!(1200), try_("JPY"));
        let kasapay = to_kasapay_money(money).expect("JPY is one of the nine");
        assert_eq!(kasapay.minor_units(), 1200);
        assert_eq!(from_kasapay_money(kasapay).expect("round trips"), money);
    }

    #[test]
    fn round_trips_kwd_with_three_decimal_places() {
        let money = Money::new(dec!(3.500), try_("KWD"));
        let kasapay = to_kasapay_money(money).expect("KWD is one of the nine");
        assert_eq!(kasapay.minor_units(), 3500);
        assert_eq!(from_kasapay_money(kasapay).expect("round trips"), money);
    }

    #[test]
    fn a_currency_kasapay_does_not_know_is_an_explicit_error_not_a_silent_round() {
        let money = Money::new(dec!(10), try_("PLN"));
        let err = to_kasapay_money(money).expect_err("PLN is not one of kasapay's nine");
        assert_eq!(err.code(), "invalid");
    }

    // -----------------------------------------------------------------
    // capture / cancel / refund / lookup — each call reaches a fake
    // kasapay Provider carrying what tezgah asked for.
    // -----------------------------------------------------------------

    #[derive(Debug, Default)]
    struct FakeKasapay {
        captured: Mutex<Vec<(String, Option<KMoney>)>>,
        canceled: Mutex<Vec<String>>,
        refunded: Mutex<Vec<RefundRequest>>,
        looked_up: Mutex<Vec<String>>,
        lookup_answer: Mutex<Option<LookupAnswer>>,
    }

    #[derive(Debug)]
    enum LookupAnswer {
        None,
        Some(Charge),
        Err,
    }

    fn a_charge() -> Charge {
        Charge {
            id: Some(PaymentId::issued("pay_1")),
            order: None,
            amount: KMoney::from_minor_units(1000, KCurrency::Try),
            order_amount: None,
            status: Status::Authorized,
            next_action: None,
            provider: ProviderId::new("fake"),
            raw: Raw::from_json(&serde_json::json!({ "id": "pay_1" })),
        }
    }

    #[async_trait]
    impl kasapay_core::Provider for FakeKasapay {
        fn id(&self) -> ProviderId {
            ProviderId::new("fake")
        }

        async fn charge(&self, _request: &ChargeRequest) -> std::result::Result<Charge, KError> {
            Err(KError::new(
                KErrorKind::Unsupported,
                self.id(),
                "not exercised",
            ))
        }

        async fn charge_status(&self, _id: &PaymentId) -> std::result::Result<Charge, KError> {
            Err(KError::new(
                KErrorKind::Unsupported,
                self.id(),
                "not exercised",
            ))
        }

        async fn capture(
            &self,
            id: &PaymentId,
            amount: Option<KMoney>,
            _idempotency: Option<&kasapay_core::IdempotencyKey>,
        ) -> std::result::Result<Charge, KError> {
            self.captured
                .lock()
                .expect("lock")
                .push((id.as_str().to_owned(), amount));
            Ok(Charge {
                status: Status::Captured,
                amount: amount.unwrap_or(KMoney::from_minor_units(0, KCurrency::Try)),
                ..a_charge()
            })
        }

        async fn cancel(&self, id: &PaymentId) -> std::result::Result<Charge, KError> {
            self.canceled
                .lock()
                .expect("lock")
                .push(id.as_str().to_owned());
            Ok(Charge {
                status: Status::Canceled,
                ..a_charge()
            })
        }

        async fn refund(&self, request: &RefundRequest) -> std::result::Result<Refund, KError> {
            self.refunded.lock().expect("lock").push(request.clone());
            Ok(Refund {
                id: None,
                payment: request.payment.clone(),
                amount: request
                    .amount
                    .unwrap_or(KMoney::from_minor_units(0, KCurrency::Try)),
                status: RefundStatus::Succeeded,
                next_action: None,
                provider: self.id(),
                raw: Raw::from_json(&serde_json::json!({ "refunded": true })),
            })
        }

        async fn lookup(&self, order: &OrderRef) -> std::result::Result<Option<Charge>, KError> {
            self.looked_up
                .lock()
                .expect("lock")
                .push(order.as_str().to_owned());
            match self.lookup_answer.lock().expect("lock").take() {
                Some(LookupAnswer::None) | None => Ok(None),
                Some(LookupAnswer::Some(charge)) => Ok(Some(charge)),
                Some(LookupAnswer::Err) => Err(KError::new(
                    KErrorKind::Transport,
                    self.id(),
                    "the question went unanswered",
                )),
            }
        }

        async fn instruments(
            &self,
            _customer: &str,
        ) -> std::result::Result<Vec<Instrument>, KError> {
            Err(KError::new(
                KErrorKind::Unsupported,
                self.id(),
                "not exercised",
            ))
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                separate_capture: true,
                partial_capture: true,
                partial_refund: true,
                repeated_refund: true,
                lookup_by_order: true,
                saved_instruments: false,
            }
        }
    }

    #[tokio::test]
    async fn capture_reaches_the_kasapay_call_with_the_right_id_and_amount() {
        let fake = FakeKasapay::default();
        let amount = Money::new(dec!(10.00), try_("TRY"));

        let result = capture(&fake, "pay_1", amount, Some("idem-1"))
            .await
            .expect("captures");

        assert_eq!(result.amount, amount);
        let calls = fake.captured.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "pay_1");
        assert_eq!(
            calls[0].1,
            Some(KMoney::from_minor_units(1000, KCurrency::Try))
        );
    }

    #[tokio::test]
    async fn cancel_reaches_the_kasapay_call_with_the_right_id() {
        let fake = FakeKasapay::default();

        cancel(&fake, "pay_2").await.expect("cancels");

        assert_eq!(
            *fake.canceled.lock().expect("lock"),
            vec!["pay_2".to_owned()]
        );
    }

    #[tokio::test]
    async fn refund_reaches_the_kasapay_call_with_the_right_id_and_amount() {
        let fake = FakeKasapay::default();
        let amount = Money::new(dec!(4.25), try_("TRY"));

        let result = refund(&fake, "pay_3", amount, None).await.expect("refunds");

        assert_eq!(result.amount, amount);
        let calls = fake.refunded.lock().expect("lock");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].payment.as_str(), "pay_3");
        assert_eq!(
            calls[0].amount,
            Some(KMoney::from_minor_units(425, KCurrency::Try))
        );
    }

    #[tokio::test]
    async fn lookup_carries_nothing_taken_separately_from_what_happened_and_from_unanswered() {
        let fake = FakeKasapay::default();

        *fake.lookup_answer.lock().expect("lock") = Some(LookupAnswer::None);
        assert!(lookup(&fake, "order-1").await.expect("answers").is_none());

        *fake.lookup_answer.lock().expect("lock") = Some(LookupAnswer::Some(a_charge()));
        let found = lookup(&fake, "order-1")
            .await
            .expect("answers")
            .expect("something happened to it");
        assert_eq!(found.status, AuthorizationStatus::Authorized);

        *fake.lookup_answer.lock().expect("lock") = Some(LookupAnswer::Err);
        assert!(lookup(&fake, "order-1").await.is_err());

        assert_eq!(
            *fake.looked_up.lock().expect("lock"),
            vec!["order-1".to_owned(); 3],
        );
    }
}
