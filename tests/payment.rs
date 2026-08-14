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
use tezgah::id::{PaymentId, PaymentSessionId};
use tezgah::money::{Currency, Money};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, Authorized, CancelRequest,
    CaptureRequest, CaptureResult, CollectionStatus, NewCollection, NewSession, PaymentProvider,
    RefundRequest, RefundResult, SessionRequest, SessionResponse, SessionStatus, WebhookEvent,
    WebhookKind, WebhookOutcome,
};
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
}

impl FakeProvider {
    fn approving() -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Authorized,
            holds: None,
        }
    }

    fn asking_for_more() -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::RequiresMore,
            holds: None,
        }
    }

    fn holding(amount: Money) -> FakeProvider {
        FakeProvider {
            authorization: AuthorizationStatus::Authorized,
            holds: Some(amount),
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
    match payment::capture(&mut tx, &ctx, paid, try_(dec!(100.00)), None).await {
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
    payment::capture(&mut tx, &ctx, paid, try_(dec!(100.00)), None)
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
    payment::capture(&mut tx, &ctx, paid, try_(dec!(40.00)), None)
        .await
        .expect("a partial capture");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let refused = payment::refund(&mut tx, &ctx, paid, try_(dec!(40.01)), None, None).await;
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
    let refused = payment::capture(&mut tx, &ctx, paid, try_(dec!(100.01)), None).await;
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
    payment::capture(&mut tx, &ctx, paid, try_(dec!(30.00)), None)
        .await
        .expect("the first capture");
    payment::capture(&mut tx, &ctx, paid, try_(dec!(25.50)), None)
        .await
        .expect("the second capture");
    payment::refund(&mut tx, &ctx, paid, try_(dec!(5.50)), None, None)
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

    let refused = payment::capture(&mut tx, &ctx, paid, try_(dec!(100.00)), None).await;
    assert!(
        refused.expect_err("capture to be refused").is_denied(),
        "a role that may authorise could also capture"
    );

    let refused = payment::refund(&mut tx, &ctx, paid, try_(dec!(1.00)), None, None).await;
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

    let refused = payment::capture(&mut tx, &theirs, paid, try_(dec!(1.00)), None).await;
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
