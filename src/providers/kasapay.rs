//! Vocabulary borrowed from [kasapay](https://github.com/productdevbook/kasapay) — tezgah#53.
//!
//! Not a live adapter, and nothing here is called from [`crate::checkout`] or
//! [`crate::subscription`] yet. #53's design note found that
//! [`PaymentProvider::create_session`]/`authorize` do not collapse onto
//! [`kasapay_core::Provider::charge`] uniformly across every provider, so no
//! wrapper implements [`PaymentProvider`] over `kasapay_core::Provider` here.
//! `capture`, `cancel`, `refund` and `lookup` do map close to 1:1 — that
//! mapping is what this module is, kept ready for the adapter that will call
//! it — plus the [`Money`] conversion boundary it depends on, proven lossless
//! before anything is built on top of it.
//!
//! `src/providers/stripe.rs` and `iyzico.rs` — tezgah's own hosted-flow
//! adapters, written before kasapay existed — are gone. Both waited on a
//! kasapay gap this module could not paper over: [kasapay#149] closed
//! `Currency`'s nine-variant ceiling (it names 119 now, Stripe's own list
//! among them), and [kasapay#150] gave `Provider::charge` a buyer, addresses
//! and a basket, so iyzico's classic hosted form goes through the trait
//! instead of refusing every call with `Unsupported`. Neither gap is this
//! module's to hold open any longer.
//!
//! [`PaymentProvider`]: crate::payment::PaymentProvider
//! [`PaymentProvider::create_session`]: crate::payment::PaymentProvider::create_session
//! [kasapay#149]: https://github.com/productdevbook/kasapay/issues/149
//! [kasapay#150]: https://github.com/productdevbook/kasapay/issues/150

use std::str::FromStr;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::error::{Error, Result};
use crate::money::{Currency, Money};
use crate::payment::{Authorization, AuthorizationStatus, CaptureResult, RefundResult};

/// Converts a tezgah amount into kasapay's minor-unit [`kasapay_core::Money`].
///
/// `exponent` is the shop's own — [`crate::store::exponent`], the one place
/// this crate reads "how many decimal places does this currency have"
/// (tezgah#94, tezgah#171). `kasapay_core::Currency::exponent()` is read here
/// for one thing only, same as the module doc says: to answer whether kasapay
/// knows the currency at all. It is not consulted for rounding, because
/// `Money::allocate` was already rounded at the shop's exponent before this
/// function sees a part, and re-rounding each part at a second, possibly
/// different exponent is how parts stop summing to the whole.
///
/// A shop's `currency` row is free to configure an exponent kasapay's own
/// table disagrees with for that code — 0..=6, shop-editable, exactly per
/// `store.rs`'s doc. This function does not adjudicate that disagreement by
/// picking a side; it errors, because a mismatch here means kasapay will
/// decode the minor units it is handed by its own fixed exponent, not the
/// shop's, and silently proceeding would send a shop's money at a scale
/// nothing downstream agreed to.
///
/// Also errors rather than rounding or dropping when kasapay does not know
/// the currency at all: kasapay's `Currency` is closed — 119 variants as of
/// kasapay 0.0.5, not open like tezgah's own — and a shop selling outside
/// them cannot be represented in a `ChargeRequest`. What is left outside that
/// list is currencies no provider here settles in, or ones ISO and a
/// provider's own reading disagree about closely enough that kasapay declines
/// to guess — see this module's own doc.
pub fn to_kasapay_money(money: Money, exponent: u32) -> Result<kasapay_core::Money> {
    let currency = kasapay_core::Currency::from_str(money.currency.as_str()).map_err(|_| {
        Error::invalid(format!("kasapay does not know currency {}", money.currency))
    })?;
    let kasapay_exponent = currency.exponent();
    if exponent != kasapay_exponent {
        return Err(Error::invalid(format!(
            "{} is configured at {exponent} decimal places but kasapay settles it at \
             {kasapay_exponent} — reconcile the shop's currency row before routing it \
             through kasapay",
            money.currency
        )));
    }
    let scale = Decimal::from(10u64.pow(exponent));
    let minor = (money.amount.round_dp(exponent) * scale)
        .round()
        .to_i64()
        .ok_or_else(|| Error::invalid("that amount does not fit kasapay's minor units"))?;
    Ok(kasapay_core::Money::from_minor_units(minor, currency))
}

/// The inverse of [`to_kasapay_money`], for reading an amount back out of
/// kasapay's answer. `exponent` is the same shop exponent the outgoing call
/// used, for the same reason: kasapay's minor units are decoded at the scale
/// they were encoded at, not re-derived from kasapay's own table.
///
/// `kasapay_core::Currency::code()` always writes three ASCII letters, so
/// [`Currency::parse`] cannot fail on it in practice — but this still reports
/// rather than assumes, because a library does not panic on a fact about
/// another crate that only holds today.
pub fn from_kasapay_money(money: kasapay_core::Money, exponent: u32) -> Result<Money> {
    let currency = Currency::parse(money.currency().code())
        .map_err(|_| Error::bug("kasapay named a currency tezgah could not parse"))?;
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
///
/// `exponent` is the shop's own, from [`crate::store::exponent`] — see
/// [`to_kasapay_money`] for why it is a parameter rather than read off
/// kasapay's own table.
pub async fn capture(
    provider: &dyn kasapay_core::Provider,
    payment_id: &str,
    amount: Money,
    exponent: u32,
    idempotency_key: Option<&str>,
) -> Result<CaptureResult> {
    let id = kasapay_core::PaymentId::issued(payment_id);
    let minor = to_kasapay_money(amount, exponent)?;
    let key = idempotency_key.map(kasapay_core::IdempotencyKey::new);
    let charge = provider
        .capture(&id, Some(minor), key.as_ref())
        .await
        .map_err(from_kasapay_error)?;
    Ok(CaptureResult {
        amount: from_kasapay_money(charge.amount, exponent)?,
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
///
/// `exponent` is the shop's own, from [`crate::store::exponent`] — see
/// [`to_kasapay_money`] for why it is a parameter rather than read off
/// kasapay's own table.
pub async fn refund(
    provider: &dyn kasapay_core::Provider,
    payment_id: &str,
    amount: Money,
    exponent: u32,
    idempotency_key: Option<&str>,
) -> Result<RefundResult> {
    let id = kasapay_core::PaymentId::issued(payment_id);
    let minor = to_kasapay_money(amount, exponent)?;
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
        amount: from_kasapay_money(refund.amount, exponent)?,
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
///
/// `exponent` is the shop's own, from [`crate::store::exponent`] for the
/// currency the caller expects this order back in — see [`to_kasapay_money`]
/// for why it is a parameter rather than read off kasapay's own table.
pub async fn lookup(
    provider: &dyn kasapay_core::Provider,
    order_ref: &str,
    exponent: u32,
) -> Result<Option<Authorization>> {
    let order = kasapay_core::OrderRef::new(order_ref.to_owned());
    match provider.lookup(&order).await {
        Ok(None) => Ok(None),
        Ok(Some(charge)) => Ok(Some(to_authorization(charge, exponent)?)),
        Err(err) => Err(from_kasapay_error(err)),
    }
}

/// kasapay's [`kasapay_core::Status`] is six variants and `#[non_exhaustive]`;
/// tezgah's [`AuthorizationStatus`] is four, also `#[non_exhaustive]`.
/// `Authorized` and `Captured` both become `Authorized` here — the same
/// conflation iyzico's own checkout form already needed when tezgah drove it
/// directly, confirming money already taken rather than moving it again.
///
/// `Pending` and `RequiresAction` are kept apart (tezgah#168):
/// `RequiresAction` needs a human, `Pending` needs nobody, and folding the
/// two together used to turn a renewal the provider was still processing
/// into a permanent decline the moment nobody was in a browser to act on it.
///
/// `Canceled` stays with `Failed` under `Error`: both are a closed
/// authorisation this attempt will never get money out of, and
/// [`crate::payment::Authorized::Failed`]'s "the session is closed, a new
/// one is the retry" is exactly as true of a cancelled hold as of a
/// declined one.
///
/// The wildcard is required by `Status`'s own `#[non_exhaustive]`, not by any
/// variant left unhandled above. It used to default to `RequiresMore`, which
/// off-session is a permanent decline (tezgah#169) — a seventh status
/// kasapay adds tomorrow would compile, land in this arm, and start
/// dunning contracts that may be fine, with nothing left to notice since
/// #168 made `AuthorizationStatus` itself `#[non_exhaustive]`. An unmapped
/// status is this mapping unfinished, not a business outcome, so it errors
/// the same way an unreachable arm on tezgah's own enums already does
/// elsewhere in this crate (`Error::bug`), rather than guessing at one.
fn map_status(status: kasapay_core::Status) -> Result<AuthorizationStatus> {
    use kasapay_core::Status;
    match status {
        Status::Authorized | Status::Captured => Ok(AuthorizationStatus::Authorized),
        Status::RequiresAction => Ok(AuthorizationStatus::RequiresMore),
        Status::Pending => Ok(AuthorizationStatus::Pending),
        Status::Failed | Status::Canceled => Ok(AuthorizationStatus::Error),
        _ => Err(Error::bug(
            "kasapay sent a status this mapping does not know",
        )),
    }
}

fn to_authorization(charge: kasapay_core::Charge, exponent: u32) -> Result<Authorization> {
    let redirect = match &charge.next_action {
        Some(kasapay_core::NextAction::Redirect { url, .. }) => Some(url.to_string()),
        _ => None,
    };
    Ok(Authorization {
        status: map_status(charge.status)?,
        amount: Some(from_kasapay_money(charge.amount, exponent)?),
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
        let kasapay = to_kasapay_money(money, 2).expect("kasapay knows TRY, at exponent 2");
        assert_eq!(kasapay.minor_units(), 1050);
        assert_eq!(from_kasapay_money(kasapay, 2).expect("round trips"), money);
    }

    #[test]
    fn round_trips_usd() {
        let money = Money::new(dec!(7.05), try_("USD"));
        let kasapay = to_kasapay_money(money, 2).expect("kasapay knows USD, at exponent 2");
        assert_eq!(kasapay.minor_units(), 705);
        assert_eq!(from_kasapay_money(kasapay, 2).expect("round trips"), money);
    }

    #[test]
    fn round_trips_jpy_with_no_decimal_places() {
        let money = Money::new(dec!(1200), try_("JPY"));
        let kasapay = to_kasapay_money(money, 0).expect("kasapay knows JPY, at exponent 0");
        assert_eq!(kasapay.minor_units(), 1200);
        assert_eq!(from_kasapay_money(kasapay, 0).expect("round trips"), money);
    }

    #[test]
    fn round_trips_kwd_with_three_decimal_places() {
        let money = Money::new(dec!(3.500), try_("KWD"));
        let kasapay = to_kasapay_money(money, 3).expect("kasapay knows KWD, at exponent 3");
        assert_eq!(kasapay.minor_units(), 3500);
        assert_eq!(from_kasapay_money(kasapay, 3).expect("round trips"), money);
    }

    // kasapay 0.0.5 (kasapay#149) took `Currency` from nine variants to 119,
    // PLN among them — a shop selling in PLN or SEK can now be routed through
    // kasapay at all, where before neither could be represented in a
    // `ChargeRequest`.
    #[test]
    fn round_trips_pln() {
        let money = Money::new(dec!(19.99), try_("PLN"));
        let kasapay = to_kasapay_money(money, 2).expect("kasapay#149: kasapay knows PLN now");
        assert_eq!(kasapay.minor_units(), 1999);
        assert_eq!(from_kasapay_money(kasapay, 2).expect("round trips"), money);
    }

    #[test]
    fn round_trips_sek() {
        let money = Money::new(dec!(249.50), try_("SEK"));
        let kasapay = to_kasapay_money(money, 2).expect("kasapay#149: kasapay knows SEK now");
        assert_eq!(kasapay.minor_units(), 24950);
        assert_eq!(from_kasapay_money(kasapay, 2).expect("round trips"), money);
    }

    #[test]
    fn a_currency_kasapay_does_not_know_is_an_explicit_error_not_a_silent_round() {
        // ISK: kasapay's own doc names this one deliberately, not an
        // oversight — Stripe treats the Icelandic krona as zero-decimal
        // where ISO calls it two, and kasapay declines to guess which
        // reading a shop wants rather than pick one silently.
        let money = Money::new(dec!(10), try_("ISK"));
        let err = to_kasapay_money(money, 0).expect_err("ISK is not one of kasapay's 119");
        assert_eq!(err.code(), "invalid");
    }

    // -----------------------------------------------------------------
    // tezgah#171 — the shop's own exponent (store::exponent) is what
    // `Money::allocate` rounded the parts by; kasapay-core hard-codes its
    // own for the currencies it settles. When a shop has configured a
    // currency at a different exponent than kasapay's, this boundary must
    // say so rather than pick one silently — kasapay decodes minor units by
    // its own fixed table, so a mismatch is an inconsistency, not a
    // rounding choice tezgah can paper over.
    // -----------------------------------------------------------------

    #[test]
    fn a_shop_exponent_that_disagrees_with_kasapays_own_is_an_explicit_error() {
        // kasapay-core hard-codes TRY at 2; a shop's `currency` row may say 3.
        let money = Money::new(dec!(10.500), try_("TRY"));
        let err = to_kasapay_money(money, 3).expect_err("TRY is 2 at kasapay, not 3");
        assert_eq!(err.code(), "invalid");
    }

    #[test]
    fn a_shop_exponent_beyond_kasapays_max_is_also_an_explicit_error() {
        // The schema allows up to 6; KWD, kasapay's deepest, is 3.
        let money = Money::new(dec!(3.500000), try_("KWD"));
        let err = to_kasapay_money(money, 6).expect_err("KWD is 3 at kasapay, not 6");
        assert_eq!(err.code(), "invalid");
    }

    #[test]
    fn from_kasapay_money_also_refuses_to_guess_when_told_a_different_exponent() {
        let kasapay = KMoney::from_minor_units(1050, KCurrency::Try);
        let at_kasapays_own = from_kasapay_money(kasapay, 2).expect("round trips");
        let at_a_different_exponent = from_kasapay_money(kasapay, 3).expect("still decodes");
        assert_ne!(at_kasapays_own, at_a_different_exponent);
    }

    #[test]
    fn allocated_parts_still_sum_to_the_whole_after_the_kasapay_boundary() {
        let exponent = 3;
        let total = Money::new(dec!(10.001), try_("KWD"));
        let weights = [dec!(1), dec!(1), dec!(1)];
        let parts = crate::money::allocate(total, &weights, exponent).expect("allocates");
        assert_eq!(parts.len(), 3);
        // The remainder lands on one part, not evenly — a second, coarser
        // rounding at the kasapay boundary is exactly what would break this.
        assert_ne!(parts[0].amount, parts[1].amount);

        let mut sum = Money::new(Decimal::ZERO, try_("KWD"));
        for part in parts {
            let kasapay = to_kasapay_money(part, exponent).expect("kasapay knows KWD");
            let back = from_kasapay_money(kasapay, exponent).expect("round trips");
            sum = sum.plus(back).expect("same currency throughout");
        }

        assert_eq!(sum, total);
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
        Some(Box<Charge>),
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

        async fn resume(&self, _continuation: &str) -> std::result::Result<Charge, KError> {
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
                Some(LookupAnswer::Some(charge)) => Ok(Some(*charge)),
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
                resume_by_continuation: false,
                saved_instruments: false,
            }
        }
    }

    #[tokio::test]
    async fn capture_reaches_the_kasapay_call_with_the_right_id_and_amount() {
        let fake = FakeKasapay::default();
        let amount = Money::new(dec!(10.00), try_("TRY"));

        let result = capture(&fake, "pay_1", amount, 2, Some("idem-1"))
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

        let result = refund(&fake, "pay_3", amount, 2, None)
            .await
            .expect("refunds");

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
        assert!(
            lookup(&fake, "order-1", 2)
                .await
                .expect("answers")
                .is_none()
        );

        *fake.lookup_answer.lock().expect("lock") = Some(LookupAnswer::Some(Box::new(a_charge())));
        let found = lookup(&fake, "order-1", 2)
            .await
            .expect("answers")
            .expect("something happened to it");
        assert_eq!(found.status, AuthorizationStatus::Authorized);

        *fake.lookup_answer.lock().expect("lock") = Some(LookupAnswer::Err);
        assert!(lookup(&fake, "order-1", 2).await.is_err());

        assert_eq!(
            *fake.looked_up.lock().expect("lock"),
            vec!["order-1".to_owned(); 3],
        );
    }

    // -----------------------------------------------------------------
    // Status — tezgah#168: `Pending` and `RequiresAction` used to fold
    // together, which inverted an off-session renewal's outcome.
    // -----------------------------------------------------------------

    #[test]
    fn a_provider_still_working_is_pending_not_requires_more() {
        use kasapay_core::Status;

        assert_eq!(
            map_status(Status::Pending).expect("known variant"),
            AuthorizationStatus::Pending
        );
        assert_eq!(
            map_status(Status::RequiresAction).expect("known variant"),
            AuthorizationStatus::RequiresMore
        );
    }

    #[test]
    fn a_cancelled_authorisation_is_a_closed_end_state_like_a_decline() {
        use kasapay_core::Status;

        // Both are a session this attempt will never get money out of;
        // `Authorized::Failed`'s "closed, a new one is the retry" is true of
        // either one.
        assert_eq!(
            map_status(Status::Canceled).expect("known variant"),
            AuthorizationStatus::Error
        );
        assert_eq!(
            map_status(Status::Failed).expect("known variant"),
            AuthorizationStatus::Error
        );
    }

    #[test]
    fn every_kasapay_status_lands_on_one_of_tezgahs_four() {
        use kasapay_core::Status;

        assert_eq!(
            map_status(Status::Authorized).expect("known variant"),
            AuthorizationStatus::Authorized
        );
        assert_eq!(
            map_status(Status::Captured).expect("known variant"),
            AuthorizationStatus::Authorized
        );
        assert_eq!(
            map_status(Status::RequiresAction).expect("known variant"),
            AuthorizationStatus::RequiresMore
        );
        assert_eq!(
            map_status(Status::Pending).expect("known variant"),
            AuthorizationStatus::Pending
        );
        assert_eq!(
            map_status(Status::Failed).expect("known variant"),
            AuthorizationStatus::Error
        );
        assert_eq!(
            map_status(Status::Canceled).expect("known variant"),
            AuthorizationStatus::Error
        );
    }

    // tezgah#169: the wildcard arm that used to default an unrecognised
    // `Status` to `RequiresMore` now errors instead. Not tested by
    // constructing one, on purpose — `Status` is `#[non_exhaustive]` and its
    // six variants above are exactly the ones this match already names, so
    // nothing outside kasapay-core can build a seventh to hand it. What is
    // tested is everything the wildcard is *not*: the six known variants
    // above still map exactly as #168 decided.
}
