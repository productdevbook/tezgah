//! What must hold when money moves.
//!
//! The provider is a fake rather than a network: what is being tested is not
//! whether Stripe works, it is whether a second delivery of the same webhook
//! can take the money twice, whether a refund can exceed what was captured, and
//! whether capturing asks a permission that authorising did not.

mod common;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::Shop;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::id::{OrderId, PaymentId, PaymentSessionId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, Authorized, CancelRequest,
    CaptureRequest, CaptureResult, CollectionStatus, Installment, NewCollection, NewSession,
    PaymentProvider, RefundRequest, RefundResult, SessionRequest, SessionResponse, SessionStatus,
    SurchargeBearer, WebhookEvent, WebhookKind, WebhookOutcome,
};
use tezgah::ports::Ctx;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, Host, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use tezgah::{Error, Paging, Result};

const PROVIDER: &str = "fake";

/// A provider that does whatever the test asked it to, so the trait is
/// exercised without a network.
#[derive(Debug)]
struct FakeProvider {
    authorization: AuthorizationStatus,
    /// What the provider claims it held, when that is not what it was asked for.
    holds: Option<Money>,
    /// The plan the bank accepted, when the card was split.
    plan: Option<Installment>,
}

impl FakeProvider {
    fn approving() -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Authorized,
            holds: None,
            plan: None,
        }
    }

    fn asking_for_more() -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::RequiresMore,
            holds: None,
            plan: None,
        }
    }

    fn still_processing() -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Pending,
            holds: None,
            plan: None,
        }
    }

    fn holding(amount: Money) -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Authorized,
            holds: Some(amount),
            plan: None,
        }
    }

    /// A card split into `count`, charged `charged`, with `surcharge` of that
    /// carried by whoever `bearer` says.
    fn on_instalments(
        charged: Money,
        count: i32,
        surcharge: Money,
        bearer: SurchargeBearer,
    ) -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Authorized,
            holds: Some(charged),
            plan: Some(Installment {
                count,
                surcharge,
                bearer,
                campaign: Some("a-campaign".to_owned()),
            }),
        }
    }
}

#[async_trait]
impl PaymentProvider for FakeProvider {
    fn code(&self) -> &'static str {
        PROVIDER
    }

    async fn create_session(&self, req: SessionRequest) -> Result<SessionResponse> {
        Ok(SessionResponse {
            data: json!({ "intent": req.session_id.to_string() }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> Result<Authorization> {
        Ok(Authorization {
            status: self.authorization,
            amount: Some(self.holds.unwrap_or(req.amount)),
            data: json!({ "intent": req.session_id.to_string() }),
            redirect: match self.authorization {
                AuthorizationStatus::RequiresMore => Some("https://example.test/3ds".to_owned()),
                _ => None,
            },
            message: None,
            installment: self.plan.clone(),
        })
    }

    async fn capture(&self, req: CaptureRequest) -> Result<CaptureResult> {
        Ok(CaptureResult {
            amount: req.amount,
            data: json!({ "captured": true }),
        })
    }

    async fn refund(&self, req: RefundRequest) -> Result<RefundResult> {
        Ok(RefundResult {
            amount: req.amount,
            data: json!({ "refunded": true }),
        })
    }

    async fn cancel(&self, _req: CancelRequest) -> Result<()> {
        Ok(())
    }

    fn parse_webhook(&self, headers: &[(String, String)], body: &[u8]) -> Result<WebhookEvent> {
        let signed = headers
            .iter()
            .any(|(name, value)| name == "fake-signature" && value == "good");
        if !signed {
            return Err(Error::provider(PROVIDER, "that signature is not ours"));
        }

        let payload: serde_json::Value =
            serde_json::from_slice(body).map_err(|_| Error::provider(PROVIDER, "not json"))?;

        Ok(WebhookEvent {
            event_id: payload["id"].as_str().unwrap_or_default().to_owned(),
            kind: WebhookKind::Captured,
            event_type: "payment.captured".to_owned(),
            session_id: None,
            amount: None,
            payload,
        })
    }
}

/// A host that allows everything except moving money, for proving that
/// capturing asks separately from authorising.
#[derive(Debug, Default)]
struct Clerk;

impl Authorizer for Clerk {
    fn authorize(&self, _: &Actor, action: Action, _: &Resource) -> Result<Permit> {
        if action == Action::Settle {
            Err(Error::denied())
        } else {
            Ok(Permit::granted())
        }
    }
}

impl Clock for Clerk {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for Clerk {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for Clerk {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for Clerk {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> Result<()> {
        Ok(())
    }
}

fn try_(amount: rust_decimal::Decimal) -> Money {
    Money::new(amount, Currency::parse("TRY").expect("a currency code"))
}

/// A registered provider, a collection for `total`, and an open session on it.
async fn open_session(shop: &Shop, total: Money) -> PaymentSessionId {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    payment::register_provider(&mut tx, &ctx, PROVIDER)
        .await
        .expect("a provider");

    let collection = payment::create_collection(
        &mut tx,
        &ctx,
        NewCollection {
            amount: total,
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a collection");

    let session = payment::create_session(
        &mut tx,
        &ctx,
        NewSession {
            collection_id: collection.id,
            provider_code: PROVIDER.to_owned(),
            amount: total,
            context: None,
            installment_count: None,
        },
    )
    .await
    .expect("a session");

    tx.commit().await.expect("to commit");
    session.id
}

/// The whole authorise dance: the provider is asked outside the transaction,
/// and what it said is written inside one.
async fn authorize(shop: &Shop, provider: &FakeProvider, session: PaymentSessionId) -> Authorized {
    let ctx = shop.ctx();

    let asked = {
        let mut tx = shop.begin().await;
        let row = payment::session(&mut tx, &ctx, session)
            .await
            .expect("the session");
        tx.commit().await.expect("to commit");
        row.money().expect("an amount")
    };

    let answer = provider
        .authorize(AuthorizeRequest {
            session_id: session,
            amount: asked,
            data: json!({}),
            context: json!({}),
            installment_count: None,
        })
        .await
        .expect("the provider to answer");

    let mut tx = shop.begin().await;
    let outcome = payment::authorize(&mut tx, &ctx, session, answer)
        .await
        .expect("to record the authorisation");
    tx.commit().await.expect("to commit");
    outcome
}

async fn authorized_payment(shop: &Shop, total: Money) -> PaymentId {
    let session = open_session(shop, total).await;
    authorize(shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id
}

/// One delivery of a webhook, from signature to acknowledgement.
///
/// The event is recorded and committed on its own before anything acts on it,
/// so a failure leaves the event as work rather than erasing it. Returns
/// whether the caller did any work at all — a redelivery must do none.
async fn deliver(shop: &Shop, provider: &FakeProvider, body: &str, paid: PaymentId) -> bool {
    let ctx = shop.ctx();
    let headers = vec![("fake-signature".to_owned(), "good".to_owned())];
    let event = provider
        .parse_webhook(&headers, body.as_bytes())
        .expect("a signed event");

    let mut tx = shop.begin().await;
    let outcome = payment::record_webhook(&mut tx, &ctx, PROVIDER, &event)
        .await
        .expect("to record the delivery");
    tx.commit().await.expect("to commit the delivery");

    let id = match outcome {
        WebhookOutcome::AlreadySeen => return false,
        WebhookOutcome::Fresh { id } => id,
    };

    let mut tx = shop.begin().await;
    match payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.00)), None).await {
        Ok(_) => {
            payment::mark_processed(&mut tx, &ctx, id)
                .await
                .expect("to mark it done");
            tx.commit().await.expect("to commit");
        }
        Err(err) => {
            tx.rollback().await.expect("to roll back the failed work");
            let mut tx = shop.begin().await;
            payment::mark_failed(&mut tx, &ctx, id, &err.to_string())
                .await
                .expect("to note the failure");
            tx.commit().await.expect("to commit");
        }
    }

    true
}

#[tokio::test]
async fn the_same_webhook_delivered_twice_captures_once() {
    let shop = Shop::open().await;
    let paid = authorized_payment(&shop, try_(dec!(100.00))).await;
    let provider = FakeProvider::approving();
    let body = r#"{"id": "evt_1", "type": "payment.captured"}"#;

    assert!(deliver(&shop, &provider, body, paid).await);
    assert!(
        !deliver(&shop, &provider, body, paid).await,
        "the second delivery did work"
    );

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    let balance = payment::balance(&mut tx, &ctx, paid)
        .await
        .expect("a balance");
    tx.commit().await.expect("to commit");

    assert_eq!(
        balance.captured,
        dec!(100.00),
        "the same event captured twice"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_capture_arriving_before_its_authorisation_waits_to_be_replayed() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let provider = FakeProvider::approving();

    // The session exists and nothing has authorised it, so no payment does yet.
    let session = open_session(&shop, try_(dec!(100.00))).await;
    let absent = PaymentId::new();
    let body = r#"{"id": "evt_early", "type": "payment.captured"}"#;
    assert!(deliver(&shop, &provider, body, absent).await);

    let mut tx = shop.begin().await;
    let waiting = payment::unprocessed(&mut tx, &ctx, Paging::first(10))
        .await
        .expect("the unprocessed events");
    tx.commit().await.expect("to commit");

    assert_eq!(waiting.len(), 1, "the early event was not kept as work");
    assert_eq!(waiting.items[0].attempts, 1);
    let event = waiting.items[0].id;

    let paid = authorize(&shop, &provider, session)
        .await
        .payment()
        .expect("a payment")
        .id;

    // Replaying is what applies it. A redelivery would not: the event id is
    // known by now, and refusing it twice is the whole point of the index.
    assert!(
        !deliver(&shop, &provider, body, paid).await,
        "a redelivery of a known event did work"
    );

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.00)), None)
        .await
        .expect("the replay to capture");
    payment::mark_processed(&mut tx, &ctx, event)
        .await
        .expect("to mark it done");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let left = payment::unprocessed(&mut tx, &ctx, Paging::first(10))
        .await
        .expect("the unprocessed events");
    let balance = payment::balance(&mut tx, &ctx, paid)
        .await
        .expect("a balance");
    tx.commit().await.expect("to commit");

    assert!(left.is_empty(), "the replayed event is still outstanding");
    assert_eq!(balance.captured, dec!(100.00));

    shop.close().await;
}

#[tokio::test]
async fn more_cannot_be_refunded_than_was_captured() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let paid = authorized_payment(&shop, try_(dec!(100.00))).await;

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(40.00)), None)
        .await
        .expect("a partial capture");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let refused = payment::refund_only(&mut tx, &ctx, paid, try_(dec!(40.01)), None, None).await;
    tx.rollback().await.expect("to roll back");

    let err = refused.expect_err("refunding more than was taken");
    assert!(err.is_conflict(), "refused as {}", err.code());

    let mut tx = shop.begin().await;
    let balance = payment::balance(&mut tx, &ctx, paid)
        .await
        .expect("a balance");
    tx.commit().await.expect("to commit");
    assert_eq!(balance.refunded, dec!(0));

    shop.close().await;
}

#[tokio::test]
async fn nothing_can_be_captured_beyond_what_was_authorised() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let paid = authorized_payment(&shop, try_(dec!(100.00))).await;

    let mut tx = shop.begin().await;
    let refused = payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.01)), None).await;
    tx.rollback().await.expect("to roll back");

    assert!(
        refused
            .expect_err("capturing beyond the hold")
            .is_conflict(),
        "capturing more than was authorised was allowed"
    );

    shop.close().await;
}

#[tokio::test]
async fn partial_captures_and_partial_refunds_add_up() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(30.00)), None)
        .await
        .expect("the first capture");
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(25.50)), None)
        .await
        .expect("the second capture");
    payment::refund_only(&mut tx, &ctx, paid, try_(dec!(5.50)), None, None)
        .await
        .expect("a partial refund");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let balance = payment::balance(&mut tx, &ctx, paid)
        .await
        .expect("a balance");
    let session = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session");
    let collection = payment::collection(&mut tx, &ctx, session.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    assert_eq!(balance.captured, dec!(55.50));
    assert_eq!(balance.refunded, dec!(5.50));
    assert_eq!(balance.refundable(), dec!(50.00));
    assert_eq!(balance.capturable(), dec!(44.50));

    assert_eq!(collection.captured(), dec!(55.50));
    assert_eq!(collection.refunded(), dec!(5.50));
    assert_eq!(collection.status(), CollectionStatus::PartiallyRefunded);

    shop.close().await;
}

#[tokio::test]
async fn capturing_asks_a_permission_authorising_did_not() {
    let shop = Shop::open().await;
    let clerk = Clerk;
    let ctx = shop.ctx_as(Actor::System, &clerk as &dyn Host);

    let session = open_session(&shop, try_(dec!(100.00))).await;
    let answer = FakeProvider::approving()
        .authorize(AuthorizeRequest {
            session_id: session,
            amount: try_(dec!(100.00)),
            data: json!({}),
            context: json!({}),
            installment_count: None,
        })
        .await
        .expect("the provider to answer");

    let mut tx = shop.begin().await;
    let paid = payment::authorize(&mut tx, &ctx, session, answer)
        .await
        .expect("authorising to be allowed a Write")
        .payment()
        .expect("a payment")
        .id;

    let refused = payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.00)), None).await;
    assert!(
        refused.expect_err("capture to be refused").is_denied(),
        "a role that may authorise could also capture"
    );

    let refused = payment::refund_only(&mut tx, &ctx, paid, try_(dec!(1.00)), None, None).await;
    assert!(refused.expect_err("refund to be refused").is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_provider_that_holds_the_wrong_amount_is_recorded_rather_than_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;

    let paid = authorize(&shop, &FakeProvider::holding(try_(dec!(120.00))), session)
        .await
        .payment()
        .expect("the money that moved to be recorded");

    assert_eq!(paid.amount, dec!(120.00), "the amount held was not kept");

    let mut tx = shop.begin().await;
    let collection = payment::collection(&mut tx, &ctx, paid.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    assert_eq!(collection.status(), CollectionStatus::Mismatch);
    assert!(
        shop.host.emitted("payment.amount_mismatch"),
        "nobody was told about the disagreement"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_session_that_needs_a_second_factor_is_left_open() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;

    let outcome = authorize(&shop, &FakeProvider::asking_for_more(), session).await;
    assert!(
        outcome.requires_more(),
        "the session was closed by a 3-D Secure step"
    );

    let mut tx = shop.begin().await;
    let waiting = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session");
    tx.commit().await.expect("to commit");
    assert_eq!(waiting.status(), SessionStatus::RequiresMore);
    assert!(waiting.status().is_open());

    // The retry is the same session authorising again, not a new one.
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment on the retry");
    assert_eq!(paid.amount, dec!(100.00));

    let mut tx = shop.begin().await;
    let done = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session");
    tx.commit().await.expect("to commit");
    assert_eq!(done.status(), SessionStatus::Authorized);

    shop.close().await;
}

/// tezgah#168: unlike `RequiresMore`, `Pending` is not a reason to send the
/// shopper anywhere — the provider itself is still working.
#[tokio::test]
async fn a_provider_still_working_is_pending_not_requires_more() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;

    let outcome = authorize(&shop, &FakeProvider::still_processing(), session).await;
    assert!(
        outcome.is_pending(),
        "a provider still working was read as needing the shopper"
    );
    assert!(!outcome.requires_more());

    let mut tx = shop.begin().await;
    let waiting = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session");
    tx.commit().await.expect("to commit");
    assert_eq!(waiting.status(), SessionStatus::Pending);
    assert!(waiting.status().is_open());

    // The retry is the same session authorising again, not a new one.
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment once the provider actually answers");
    assert_eq!(paid.amount, dec!(100.00));

    shop.close().await;
}

#[tokio::test]
async fn authorising_the_same_session_twice_leaves_one_payment() {
    let shop = Shop::open().await;
    let session = open_session(&shop, try_(dec!(100.00))).await;

    let first = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;
    let second = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;

    assert_eq!(
        first, second,
        "a second authorisation made a second payment"
    );

    shop.close().await;
}

#[tokio::test]
async fn another_scope_sees_none_of_this() {
    let shop = Shop::open().await;
    let paid = authorized_payment(&shop, try_(dec!(100.00))).await;
    let theirs = shop.theirs();

    let mut tx = shop.begin_as(shop.elsewhere).await;
    let missing = payment::payment(&mut tx, &theirs, paid).await;
    assert!(
        missing.expect_err("somebody else's payment").is_not_found(),
        "another scope could read the payment"
    );

    let missing = payment::balance(&mut tx, &theirs, paid).await;
    assert!(missing.expect_err("somebody else's balance").is_not_found());

    let refused = payment::capture_only(&mut tx, &theirs, paid, try_(dec!(1.00)), None).await;
    assert!(
        refused.expect_err("somebody else's money").is_not_found(),
        "another scope could capture the payment"
    );
    tx.rollback().await.expect("to roll back");

    shop.close().await;
}

#[tokio::test]
async fn an_unsigned_webhook_never_reaches_the_table() {
    let shop = Shop::open().await;
    let provider = FakeProvider::approving();

    let refused = provider.parse_webhook(
        &[("fake-signature".to_owned(), "forged".to_owned())],
        br#"{"id": "evt_forged"}"#,
    );
    assert_eq!(
        refused.expect_err("a forged signature").code(),
        "provider",
        "a forged signature was accepted"
    );

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    let waiting = payment::unprocessed(&mut tx, &ctx, Paging::first(10))
        .await
        .expect("the unprocessed events");
    tx.commit().await.expect("to commit");
    assert!(waiting.is_empty());

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Two writers, at the same time
// ---------------------------------------------------------------------------
//
// The rule these prove is not the arithmetic — `more_cannot_be_refunded_than_
// was_captured` already proves that sequentially. It is that the arithmetic
// still holds when both transactions are open together, which is the only
// state in which the `for update` on the payment row does any work at all.

/// One capture on its own transaction, committed if it was taken and rolled
/// back if it was refused, so the loser leaves nothing behind.
async fn capture_on(
    mut tx: Tx<'static>,
    ctx: Ctx<'_>,
    paid: PaymentId,
    amount: Money,
) -> Result<()> {
    match payment::capture_only(&mut tx, &ctx, paid, amount, None).await {
        Ok(_) => {
            tx.commit().await.map_err(Error::from)?;
            Ok(())
        }
        Err(err) => {
            tx.rollback().await.map_err(Error::from)?;
            Err(err)
        }
    }
}

async fn refund_on(
    mut tx: Tx<'static>,
    ctx: Ctx<'_>,
    paid: PaymentId,
    amount: Money,
) -> Result<()> {
    match payment::refund_only(&mut tx, &ctx, paid, amount, None, None).await {
        Ok(_) => {
            tx.commit().await.map_err(Error::from)?;
            Ok(())
        }
        Err(err) => {
            tx.rollback().await.map_err(Error::from)?;
            Err(err)
        }
    }
}

/// The ledger read back: what the capture and refund rows add up to, and what
/// the collection claims they add up to.
async fn ledger(
    shop: &Shop,
    paid: PaymentId,
    session: PaymentSessionId,
) -> (
    payment::Balance,
    rust_decimal::Decimal,
    rust_decimal::Decimal,
) {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    let balance = payment::balance(&mut tx, &ctx, paid)
        .await
        .expect("a balance");
    let session = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session");
    let collection = payment::collection(&mut tx, &ctx, session.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    (
        balance,
        collection.captured_amount.unwrap_or_default(),
        collection.refunded_amount.unwrap_or_default(),
    )
}

#[tokio::test]
async fn two_captures_race_for_the_last_of_the_hold() {
    let shop = Shop::open().await;
    let session = open_session(&shop, try_(dec!(100.00))).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;

    let one = shop.begin().await;
    let two = shop.begin().await;

    let (first, second) = tokio::join!(
        capture_on(one, shop.ctx(), paid, try_(dec!(60.00))),
        capture_on(two, shop.ctx(), paid, try_(dec!(60.00))),
    );

    let losers: Vec<_> = [&first, &second]
        .into_iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();
    assert_eq!(
        losers.len(),
        1,
        "both captures of 60 were taken against a hold of 100"
    );
    assert!(
        losers[0].is_conflict(),
        "the second capture was refused as {}",
        losers[0].code()
    );

    let (balance, captured, refunded) = ledger(&shop, paid, session).await;
    assert_eq!(balance.captured, dec!(60.00));
    assert!(
        balance.captured <= balance.authorized,
        "{} was taken against a hold of {}",
        balance.captured,
        balance.authorized
    );
    assert_eq!(captured, dec!(60.00), "the collection lost an update");
    assert_eq!(refunded, dec!(0));

    shop.close().await;
}

#[tokio::test]
async fn two_refunds_race_for_what_was_captured() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.00)), None)
        .await
        .expect("the money to be taken");
    tx.commit().await.expect("to commit");

    let one = shop.begin().await;
    let two = shop.begin().await;

    let (first, second) = tokio::join!(
        refund_on(one, shop.ctx(), paid, try_(dec!(60.00))),
        refund_on(two, shop.ctx(), paid, try_(dec!(60.00))),
    );

    let losers: Vec<_> = [&first, &second]
        .into_iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();
    assert_eq!(losers.len(), 1, "120 was given back against 100 taken");
    assert!(
        losers[0].is_conflict(),
        "the second refund was refused as {}",
        losers[0].code()
    );

    let (balance, captured, refunded) = ledger(&shop, paid, session).await;
    assert_eq!(balance.refunded, dec!(60.00));
    assert!(
        balance.refunded <= balance.captured,
        "{} was refunded of {} taken",
        balance.refunded,
        balance.captured
    );
    assert_eq!(captured, dec!(100.00));
    assert_eq!(refunded, dec!(60.00), "the collection lost an update");

    shop.close().await;
}

/// Both of these are legal whichever order they land in, so neither is refused.
/// What is being watched is the collection: two transactions recomputing it at
/// once is how one of them ends up written over.
#[tokio::test]
async fn a_capture_and_a_refund_at_the_same_time_leave_the_ledger_whole() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(100.00))).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, try_(dec!(60.00)), None)
        .await
        .expect("the first capture");
    tx.commit().await.expect("to commit");

    let one = shop.begin().await;
    let two = shop.begin().await;

    let (captured, refunded) = tokio::join!(
        capture_on(one, shop.ctx(), paid, try_(dec!(40.00))),
        refund_on(two, shop.ctx(), paid, try_(dec!(60.00))),
    );
    captured.expect("the rest of the hold to be capturable");
    refunded.expect("what was already taken to be refundable");

    let (balance, collection_captured, collection_refunded) = ledger(&shop, paid, session).await;
    assert_eq!(balance.captured, dec!(100.00));
    assert_eq!(balance.refunded, dec!(60.00));
    assert_eq!(
        collection_captured, balance.captured,
        "the collection and the capture rows disagree"
    );
    assert_eq!(
        collection_refunded, balance.refunded,
        "the collection and the refund rows disagree"
    );

    shop.close().await;
}

/// The bug #75 reports: a correct Turkish instalment sale, authorised for the
/// basket plus the vade farkı, was being recorded as a fraud signal.
#[tokio::test]
async fn an_agreed_instalment_difference_is_not_a_mismatch() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(1000.00))).await;

    let provider = FakeProvider::on_instalments(
        try_(dec!(1090.00)),
        3,
        try_(dec!(90.00)),
        SurchargeBearer::Customer,
    );
    let paid = authorize(&shop, &provider, session)
        .await
        .payment()
        .expect("a payment");

    assert_eq!(paid.amount, dec!(1090.00), "the card is charged the plan");
    assert_eq!(paid.installment_count, Some(3));
    assert_eq!(paid.surcharge_amount, dec!(90.00));
    assert_eq!(
        paid.goods_amount(),
        dec!(1000.00),
        "the goods are still worth what the basket came to"
    );

    let mut tx = shop.begin().await;
    let collection = payment::collection(&mut tx, &ctx, paid.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    assert_ne!(
        collection.status(),
        CollectionStatus::Mismatch,
        "an instalment difference the shopper agreed to was read as fraud"
    );
    assert_eq!(collection.status(), CollectionStatus::Authorized);
    assert_eq!(collection.amount, dec!(1000.00), "the basket");
    assert_eq!(collection.charged(), dec!(1090.00), "the card");
    assert_eq!(collection.installment_count, Some(3));

    shop.close().await;
}

#[tokio::test]
async fn a_difference_nobody_explained_is_still_a_mismatch() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(1000.00))).await;

    // The plan accounts for 90 of it. The other 50 is unexplained.
    let provider = FakeProvider::on_instalments(
        try_(dec!(1140.00)),
        3,
        try_(dec!(90.00)),
        SurchargeBearer::Customer,
    );
    let paid = authorize(&shop, &provider, session)
        .await
        .payment()
        .expect("a payment");

    let mut tx = shop.begin().await;
    let collection = payment::collection(&mut tx, &ctx, paid.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    assert_eq!(
        collection.status(),
        CollectionStatus::Mismatch,
        "the guard stopped catching what it is for"
    );

    shop.close().await;
}

/// "6 taksit, faizsiz": the shop funds the plan, the card sees the basket, and
/// less money arrives at settlement.
#[tokio::test]
async fn a_merchant_funded_plan_charges_the_basket_and_records_what_it_cost() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(1000.00))).await;

    let provider = FakeProvider::on_instalments(
        try_(dec!(1000.00)),
        6,
        try_(dec!(60.00)),
        SurchargeBearer::Merchant,
    );
    let paid = authorize(&shop, &provider, session)
        .await
        .payment()
        .expect("a payment");

    assert_eq!(paid.amount, dec!(1000.00));
    assert_eq!(
        paid.goods_amount(),
        dec!(1000.00),
        "the shopper paid the basket; the shop gave up the difference"
    );

    let mut tx = shop.begin().await;
    let collection = payment::collection(&mut tx, &ctx, paid.payment_collection_id)
        .await
        .expect("the collection");
    tx.commit().await.expect("to commit");

    assert_eq!(collection.status(), CollectionStatus::Authorized);
    assert_eq!(collection.charged(), dec!(1000.00));
    assert_eq!(collection.surcharge_amount, dec!(0));
    assert_eq!(collection.merchant_surcharge_amount, dec!(60.00));

    shop.close().await;
}

/// A refund settles on the rail that charged, so half the goods gives half the
/// charge back — the difference included, in proportion.
#[tokio::test]
async fn a_partial_refund_gives_back_its_share_of_the_charge() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let session = open_session(&shop, try_(dec!(1000.00))).await;

    let provider = FakeProvider::on_instalments(
        try_(dec!(1090.00)),
        3,
        try_(dec!(90.00)),
        SurchargeBearer::Customer,
    );
    let paid = authorize(&shop, &provider, session)
        .await
        .payment()
        .expect("a payment");

    let share = paid
        .charge_for_goods(try_(dec!(500.00)))
        .expect("a share of the charge");
    assert_eq!(share.amount, dec!(545.00));

    let whole = paid
        .charge_for_goods(try_(dec!(1000.00)))
        .expect("the whole charge");
    assert_eq!(
        whole.amount,
        dec!(1090.00),
        "giving all the goods back gives all the money back"
    );

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid.id, try_(dec!(1090.00)), None)
        .await
        .expect("the capture");
    payment::refund_only(&mut tx, &ctx, paid.id, share, None, None)
        .await
        .expect("the refund to be within what was charged");
    tx.commit().await.expect("to commit");

    shop.close().await;
}

/// An order on the same collection as `paid`, priced at `total`.
async fn an_order_paying(
    shop: &Shop,
    session: PaymentSessionId,
    total: Money,
    paid: PaymentId,
) -> OrderId {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a currency");

    let collection = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session")
        .payment_collection_id;

    let placed = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            payment_collection_id: Some(collection),
            lines: vec![NewOrderLine::of("A thing", 1, total)],
            ..NewOrder::of(total.currency)
        },
    )
    .await
    .expect("an order");

    order::record_transaction(&mut tx, &ctx, placed.id, total, "payment", paid.as_uuid())
        .await
        .expect("the hold in the ledger");

    tx.commit().await.expect("to commit");
    placed.id
}

#[tokio::test]
async fn cancelling_an_order_gives_the_card_its_hold_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let total = try_(dec!(100.00));

    let session = open_session(&shop, total).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;
    let placed = an_order_paying(&shop, session, total, paid).await;

    let mut tx = shop.begin().await;
    let before = order::ledger(&mut tx, &ctx, placed)
        .await
        .expect("a ledger");
    assert_eq!(before.authorized.amount, dec!(100.00));

    order::cancel(&mut tx, &ctx, placed)
        .await
        .expect("the order to cancel");

    let hold = payment::payment(&mut tx, &ctx, paid)
        .await
        .expect("the payment");
    assert!(
        hold.canceled_at.is_some(),
        "the authorisation is still sitting on the card"
    );

    let after = order::ledger(&mut tx, &ctx, placed)
        .await
        .expect("a ledger");
    assert_eq!(
        after.authorized.amount,
        dec!(0),
        "the ledger still claims money is held"
    );

    // The admin clicks twice.
    order::cancel(&mut tx, &ctx, placed)
        .await
        .expect("a second cancel to be a no-op");

    tx.commit().await.expect("to commit");
    shop.close().await;
}

/// Captured money is not un-captured. The order is refused rather than
/// half-cancelled, and the refund is a decision somebody makes first.
#[tokio::test]
async fn an_order_whose_money_was_taken_is_refunded_before_it_is_cancelled() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let total = try_(dec!(100.00));

    let session = open_session(&shop, total).await;
    let paid = authorize(&shop, &FakeProvider::approving(), session)
        .await
        .payment()
        .expect("a payment")
        .id;
    let placed = an_order_paying(&shop, session, total, paid).await;

    let mut tx = shop.begin().await;
    payment::capture_only(&mut tx, &ctx, paid, total, None)
        .await
        .expect("the capture");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let refused = order::cancel(&mut tx, &ctx, placed)
        .await
        .expect_err("money taken is refunded, not cancelled");
    assert!(refused.is_conflict());
    tx.rollback().await.expect("to roll back");

    let mut tx = shop.begin().await;
    payment::refund_only(&mut tx, &ctx, paid, total, None, None)
        .await
        .expect("the refund");
    order::cancel(&mut tx, &ctx, placed)
        .await
        .expect("a refunded order to cancel");
    tx.commit().await.expect("to commit");

    shop.close().await;
}

// ---------------------------------------------------------------------------
// #193: enabling and disabling a provider
// ---------------------------------------------------------------------------

/// `register_provider`'s upsert used to preserve `is_enabled` on conflict no
/// matter what was asked for, so nothing could ever turn a provider off or
/// back on. A second provider proves the insert path still starts on; the
/// dedicated enable/disable calls prove the write actually lands and that
/// disabling one refuses only a new session, leaving what it already
/// collected alone.
#[tokio::test]
async fn a_second_provider_can_be_enabled_and_a_disabled_one_only_refuses_a_new_session() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let paid = authorized_payment(&shop, try_(dec!(100.00))).await;
    {
        let mut tx = shop.begin().await;
        payment::capture_only(&mut tx, &ctx, paid, try_(dec!(100.00)), None)
            .await
            .expect("to capture while the provider is still on");
        tx.commit().await.expect("to commit");
    }

    let mut tx = shop.begin().await;

    let second = payment::register_provider(&mut tx, &ctx, "fake-2")
        .await
        .expect("a second provider");
    assert!(second.is_enabled, "a freshly registered provider starts on");

    payment::set_provider_enabled(&mut tx, &ctx, second.id, false)
        .await
        .expect("to turn it off");
    let off = payment::provider_by_code(&mut tx, &ctx, "fake-2")
        .await
        .expect("the provider");
    assert!(!off.is_enabled, "disabling a provider did not persist");

    payment::set_provider_enabled(&mut tx, &ctx, second.id, true)
        .await
        .expect("to turn it back on");
    let on_again = payment::provider_by_code(&mut tx, &ctx, "fake-2")
        .await
        .expect("the provider");
    assert!(
        on_again.is_enabled,
        "re-enabling a provider did not persist"
    );

    let original = payment::provider_by_code(&mut tx, &ctx, PROVIDER)
        .await
        .expect("the original provider");
    payment::set_provider_enabled(&mut tx, &ctx, original.id, false)
        .await
        .expect("to turn the original provider off");

    let collection = payment::create_collection(
        &mut tx,
        &ctx,
        NewCollection {
            amount: try_(dec!(50.00)),
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a collection");

    let refused = payment::create_session(
        &mut tx,
        &ctx,
        NewSession {
            collection_id: collection.id,
            provider_code: PROVIDER.to_owned(),
            amount: try_(dec!(50.00)),
            context: None,
            installment_count: None,
        },
    )
    .await;
    assert!(refused.is_err(), "a disabled provider opened a new session");

    payment::refund_only(&mut tx, &ctx, paid, try_(dec!(10.00)), None, None)
        .await
        .expect("a payment captured before the provider was disabled still refunds");

    tx.commit().await.expect("to commit");
    shop.close().await;
}

// ---------------------------------------------------------------------------
// #194: deleting a saved card reference
// ---------------------------------------------------------------------------

/// A deleted holder no longer answers to its old lookups, and a customer who
/// tokenises the same card again with the provider gets a clean new row
/// rather than a conflict with the one they just removed.
#[tokio::test]
async fn a_deleted_account_holder_is_gone_from_its_own_lookups_but_a_new_one_can_replace_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    payment::register_provider(&mut tx, &ctx, PROVIDER)
        .await
        .expect("a provider");
    let customer = common::a_customer(&mut tx, &ctx).await;

    let holder = payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: PROVIDER.into(),
            customer_id: Some(customer),
            external_id: "cus_1".into(),
            email: Some("shopper@example.test".into()),
            data: json!({ "brand": "visa" }),
        },
    )
    .await
    .expect("an account holder");

    payment::delete_account_holder(&mut tx, &ctx, holder.id)
        .await
        .expect("to delete it");

    assert!(
        payment::account_holder_by_id(&mut tx, &ctx, holder.id)
            .await
            .expect("the lookup to run")
            .is_none(),
        "a deleted holder still answered to its own id"
    );
    assert!(
        payment::account_holder(&mut tx, &ctx, PROVIDER, customer)
            .await
            .expect("the lookup to run")
            .is_none(),
        "a deleted holder still answered to its customer and provider"
    );

    // The provider handed back a new token for the same shopper; nothing
    // about the row just scrubbed should stand in its way.
    let replacement = payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: PROVIDER.into(),
            customer_id: Some(customer),
            external_id: "cus_2".into(),
            email: Some("shopper@example.test".into()),
            data: json!({ "brand": "visa" }),
        },
    )
    .await
    .expect("a replacement account holder");
    assert_ne!(replacement.id, holder.id);

    let found = payment::account_holder(&mut tx, &ctx, PROVIDER, customer)
        .await
        .expect("the lookup to run")
        .expect("the replacement to be found");
    assert_eq!(found.id, replacement.id);

    tx.commit().await.expect("to commit");
    shop.close().await;
}
