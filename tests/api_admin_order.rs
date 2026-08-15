//! The admin surface, where the money is.
//!
//! Four claims, each against a real Postgres: capturing asks a permission that
//! reading an order does not, an order cannot be moved somewhere its state
//! machine forbids, a refund cannot exceed what was captured, and none of it is
//! visible from another shop's scope.

mod common;

use chrono::{DateTime, Utc};
use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::api::admin_order::{
    self, CapturePayment, CreateOrder, ListOrders, Listing, MoneyIn, NewLineIn, RefundPayment,
    RefundToCredit,
};
use tezgah::id::{CaptureId, OrderId, PaymentCollectionId, PaymentId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, PaymentState};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, NewCollection, NewSession, SessionResponse,
    SessionStatus,
};
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, Host, JobSpec,
    Jobs, Permit, Resource, Scope, Tx,
};
use tezgah::{Error, Result};
use uuid::Uuid;

use async_trait::async_trait;

const PROVIDER: &str = "fake";

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: Decimal) -> Money {
    Money::new(amount, lira())
}

fn try_(amount: Decimal) -> MoneyIn {
    MoneyIn {
        amount,
        currency: "TRY".to_owned(),
    }
}

/// A host that allows everything except moving money. What a clerk who may
/// correct an address but may not refund a card would be.
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

async fn seed_currency(tx: &mut Tx<'_>, scope: Scope) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

fn an_order(amount: Decimal) -> CreateOrder {
    CreateOrder {
        currency: "TRY".to_owned(),
        email: Some("shopper@example.com".into()),
        customer_id: None,
        region_id: None,
        sales_channel_id: None,
        locale: None,
        lines: vec![NewLineIn {
            variant_id: None,
            product_id: None,
            title: "A thing".into(),
            quantity: 1,
            unit_price: try_(amount),
            requires_shipping: true,
            is_tax_inclusive: false,
            discount: Decimal::ZERO,
            tax_rate: Decimal::ZERO,
            withdrawal_exclusion: None,
            is_giftcard: false,
        }],
        shipping: Vec::new(),
        metadata: None,
    }
}

/// A payment with `total` held against it, ready to be captured. The provider
/// is not called: what is under test is what tezgah does with its answer.
async fn a_held_payment(tx: &mut Tx<'_>, ctx: &Ctx<'_>, total: Money) -> PaymentId {
    payment::register_provider(tx, ctx, PROVIDER)
        .await
        .expect("a provider");

    let collection = payment::create_collection(
        tx,
        ctx,
        NewCollection {
            amount: total,
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a collection");

    let session = payment::create_session(
        tx,
        ctx,
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

    payment::record_session(
        tx,
        ctx,
        session.id,
        SessionResponse {
            data: json!({}),
            status: SessionStatus::Pending,
        },
    )
    .await
    .expect("the session to be written back");

    payment::authorize(
        tx,
        ctx,
        session.id,
        Authorization {
            status: AuthorizationStatus::Authorized,
            amount: Some(total),
            data: json!({}),
            redirect: None,
            message: None,
            installment: None,
        },
    )
    .await
    .expect("to record the authorisation")
    .payment()
    .expect("a payment")
    .id
}

#[tokio::test]
async fn a_clerk_may_read_an_order_and_may_not_capture_its_money() -> Result<()> {
    let shop = Shop::open().await;
    let clerk = Clerk;
    let ctx = shop.ctx_as(Actor::Staff { id: Uuid::now_v7() }, &clerk as &dyn Host);

    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(100.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;

    // Reading is a View and goes through.
    let seen = admin_order::get_order(&mut tx, &ctx, placed.id).await?;
    assert_eq!(seen.id, placed.id);

    let refused = admin_order::capture_payment(
        &mut tx,
        &ctx,
        held,
        CapturePayment {
            amount: try_(dec!(100.00)),
            metadata: None,
        },
    )
    .await
    .expect_err("a clerk may not take money");
    assert!(refused.is_denied());

    let refused = admin_order::refund_payment(
        &mut tx,
        &ctx,
        held,
        RefundPayment {
            amount: try_(dec!(1.00)),
            reason_id: None,
            note: None,
        },
    )
    .await
    .expect_err("a clerk may not give money back either");
    assert!(refused.is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_status_the_state_machine_forbids_is_a_conflict() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(10.00))).await?;

    admin_order::complete_order(&mut tx, &ctx, placed.id).await?;
    admin_order::archive_order(&mut tx, &ctx, placed.id).await?;

    let refused = admin_order::complete_order(&mut tx, &ctx, placed.id)
        .await
        .expect_err("an archived order has nothing after it");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_refund_cannot_exceed_what_was_captured() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;

    admin_order::capture_payment(
        &mut tx,
        &ctx,
        held,
        CapturePayment {
            amount: try_(dec!(40.00)),
            metadata: None,
        },
    )
    .await?;

    let refused = admin_order::refund_payment(
        &mut tx,
        &ctx,
        held,
        RefundPayment {
            amount: try_(dec!(40.01)),
            reason_id: None,
            note: None,
        },
    )
    .await
    .expect_err("a penny more than was taken is not there to give back");
    assert!(refused.is_conflict());

    admin_order::refund_payment(
        &mut tx,
        &ctx,
        held,
        RefundPayment {
            amount: try_(dec!(40.00)),
            reason_id: None,
            note: None,
        },
    )
    .await?;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_shop_sees_none_of_it() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(25.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(25.00))).await;
    tx.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;

    let missing = admin_order::get_order(&mut tx, &theirs, placed.id).await;
    assert!(missing.is_err(), "somebody else's order was readable");

    let missing = admin_order::get_payment(&mut tx, &theirs, held).await;
    assert!(missing.is_err(), "somebody else's payment was readable");

    let refused = admin_order::capture_payment(
        &mut tx,
        &theirs,
        held,
        CapturePayment {
            amount: try_(dec!(1.00)),
            metadata: None,
        },
    )
    .await;
    assert!(refused.is_err(), "somebody else's money was capturable");

    let empty = admin_order::list_orders(&mut tx, &theirs, ListOrders::default()).await?;
    assert!(empty.is_empty(), "somebody else's orders were listed");

    let empty = admin_order::list_returns(&mut tx, &theirs, Listing::default()).await?;
    assert!(empty.is_empty(), "somebody else's returns were listed");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The table is the contract: a route that moves money says so, and a route
/// that only reads never asks for more than it needs.
#[test]
fn the_money_routes_ask_to_settle() {
    let money_paths = [
        "/admin/payments/{id}/capture",
        "/admin/payments/{id}/refund",
    ];

    for path in money_paths {
        let route = tezgah::api::routes()
            .into_iter()
            .find(|route| route.path == path)
            .expect("the route to be declared");
        assert_eq!(
            route.action,
            Action::Settle,
            "{path} does not ask to settle"
        );
    }
}

/// An id that belongs to no order is a not-found rather than a panic.
#[tokio::test]
async fn an_order_that_is_not_there_is_not_found() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let missing = admin_order::get_order(&mut tx, &ctx, OrderId::new())
        .await
        .expect_err("no such order");
    assert!(missing.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The collection a payment belongs to, made the order's own — the shape
/// where a payment is authorised against a standalone collection and only
/// attached to an order afterwards, which is what
/// [`order::attach_payment_collection`] has to catch the ledger up on.
async fn pay_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    payment: PaymentId,
) -> PaymentCollectionId {
    let collection: Uuid = sqlx::query_scalar(
        "select payment_collection_id from payment where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(payment.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the collection");

    let collection = PaymentCollectionId::from_uuid(collection);

    order::attach_payment_collection(tx, ctx, order_id, collection)
        .await
        .expect("the order to be paying through it");

    collection
}

#[tokio::test]
async fn a_capture_reaches_the_orders_ledger_and_a_refund_takes_it_back() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(100.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;
    pay_for(&mut tx, &ctx, placed.id, held).await;

    let before = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(before.captured.amount, Decimal::ZERO);

    admin_order::capture_payment(
        &mut tx,
        &ctx,
        held,
        CapturePayment {
            amount: try_(dec!(60.00)),
            metadata: None,
        },
    )
    .await?;

    let after = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(after.captured.amount, dec!(60.00));
    assert_eq!(after.paid.amount, dec!(60.00));
    assert_eq!(after.state, PaymentState::PartiallyCaptured);

    admin_order::refund_payment(
        &mut tx,
        &ctx,
        held,
        RefundPayment {
            amount: try_(dec!(25.00)),
            reason_id: None,
            note: None,
        },
    )
    .await?;

    let back = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(back.captured.amount, dec!(60.00));
    assert_eq!(back.refunded.amount, dec!(25.00));
    assert_eq!(back.paid.amount, dec!(35.00));
    assert_eq!(back.state, PaymentState::PartiallyRefunded);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// What `order::ledger`'s `authorized` reads and what `order_transaction`
/// actually holds for `payment`/`payment_canceled` rows, side by side — the
/// two must never disagree, whichever of the two orders authorising and
/// attaching happened in.
async fn authorized_agrees_with_transactions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) {
    let ledger = order::ledger(tx, ctx, order_id).await.expect("a ledger");

    let summed: Decimal = sqlx::query_scalar(
        "select coalesce(sum(amount), 0) from order_transaction
         where scope = $1 and order_id = $2 and reference in ('payment', 'payment_canceled')",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the transactions");

    assert_eq!(
        ledger.authorized.amount, summed,
        "order::ledger disagreed with order_transaction"
    );
}

/// The attach-then-authorise order: a checkout opens the collection with the
/// order already, and [`order::record_authorization`] writes the movement
/// the moment the hold lands.
#[tokio::test]
async fn the_ledger_agrees_when_the_order_is_attached_before_it_authorises() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(70.00))).await?;

    payment::register_provider(&mut tx, &ctx, PROVIDER).await?;
    let collection = payment::create_collection(
        &mut tx,
        &ctx,
        NewCollection {
            amount: money(dec!(70.00)),
            cart_id: None,
            metadata: None,
        },
    )
    .await?;

    order::attach_payment_collection(&mut tx, &ctx, placed.id, collection.id).await?;

    let session = payment::create_session(
        &mut tx,
        &ctx,
        NewSession {
            collection_id: collection.id,
            provider_code: PROVIDER.to_owned(),
            amount: money(dec!(70.00)),
            context: None,
            installment_count: None,
        },
    )
    .await?;
    payment::record_session(
        &mut tx,
        &ctx,
        session.id,
        SessionResponse {
            data: json!({}),
            status: SessionStatus::Pending,
        },
    )
    .await?;
    let authorized = payment::authorize(
        &mut tx,
        &ctx,
        session.id,
        Authorization {
            status: AuthorizationStatus::Authorized,
            amount: Some(money(dec!(70.00))),
            data: json!({}),
            redirect: None,
            message: None,
            installment: None,
        },
    )
    .await?
    .payment()
    .expect("a payment");

    // What `checkout` and `subscription` do in the same step, right after
    // `payment::authorize`, because both already hold the order the
    // collection is attached to.
    order::record_authorization(
        &mut tx,
        &ctx,
        collection.id,
        authorized.id,
        money(dec!(70.00)),
    )
    .await?;

    authorized_agrees_with_transactions(&mut tx, &ctx, placed.id).await;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The `pay_for` shape: a payment authorised against a collection nobody has
/// attached to an order yet, attached afterwards.
/// [`order::attach_payment_collection`] is what makes the ledger catch up.
#[tokio::test]
async fn the_ledger_agrees_when_the_order_is_attached_after_it_authorises() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(45.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(45.00))).await;
    pay_for(&mut tx, &ctx, placed.id, held).await;

    authorized_agrees_with_transactions(&mut tx, &ctx, placed.id).await;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// A redelivered webhook is the same capture arriving twice, and the ledger
/// takes it once.
#[tokio::test]
async fn the_same_capture_cannot_be_written_to_the_ledger_twice() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(100.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;
    let collection = pay_for(&mut tx, &ctx, placed.id, held).await;

    let capture = CaptureId::new();
    let _ = order::record_capture(&mut tx, &ctx, collection, capture, money(dec!(10.00))).await?;

    let again = order::record_capture(&mut tx, &ctx, collection, capture, money(dec!(10.00)))
        .await
        .expect_err("the same capture landing twice");
    assert!(again.is_conflict());

    let ledger = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(ledger.captured.amount, dec!(10.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// A collection no order is paying through has no ledger to move, and that is
/// not an error.
#[tokio::test]
async fn a_capture_for_no_order_moves_no_ledger() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let held = a_held_payment(&mut tx, &ctx, money(dec!(15.00))).await;
    let collection: Uuid = sqlx::query_scalar(
        "select payment_collection_id from payment where scope = $1 and id = $2",
    )
    .bind(shop.here.0)
    .bind(held.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the collection");

    let written = order::record_capture(
        &mut tx,
        &ctx,
        PaymentCollectionId::from_uuid(collection),
        CaptureId::new(),
        money(dec!(15.00)),
    )
    .await?;
    assert!(written.is_none());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The ledger is scoped like everything else.
#[tokio::test]
async fn another_shop_cannot_read_or_move_this_ones_ledger() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(50.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(50.00))).await;
    let collection = pay_for(&mut tx, &ctx, placed.id, held).await;
    tx.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;

    let missing = order::ledger(&mut tx, &theirs, placed.id)
        .await
        .expect_err("somebody else's ledger was readable");
    assert!(missing.is_not_found());

    let written = order::record_capture(
        &mut tx,
        &theirs,
        collection,
        CaptureId::new(),
        money(dec!(50.00)),
    )
    .await?;
    assert!(written.is_none(), "somebody else's ledger took a capture");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The route used to move a status column and leave the hold on the card. It
/// cancels as an operation now, and cancelling twice is not an error.
#[tokio::test]
async fn the_cancel_route_voids_the_authorisation_and_takes_a_second_click() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(100.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;
    pay_for(&mut tx, &ctx, placed.id, held).await;

    admin_order::cancel_order(&mut tx, &ctx, placed.id).await?;

    let voided = payment::payment(&mut tx, &ctx, held).await?;
    assert!(
        voided.canceled_at.is_some(),
        "the route reported success and left the hold on the card"
    );

    admin_order::cancel_order(&mut tx, &ctx, placed.id).await?;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The route `credit::refund_to_credit` never had: money leaves the order and
/// lands on the customer's balance rather than going back to a card, and a
/// clerk who may not move money is refused the same way capture and refund
/// already are.
#[tokio::test]
async fn refund_to_credit_moves_the_order_not_the_card() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let customer = common::a_customer(&mut tx, &ctx).await;
    let mut placed_input = an_order(dec!(100.00));
    placed_input.customer_id = Some(customer);
    let placed = admin_order::create_order(&mut tx, &ctx, placed_input).await?;

    let clerk = Clerk;
    let clerk_ctx = shop.ctx_as(Actor::Staff { id: Uuid::now_v7() }, &clerk as &dyn Host);
    let refused = admin_order::refund_order_to_credit(
        &mut tx,
        &clerk_ctx,
        placed.id,
        RefundToCredit {
            amount: try_(dec!(40.00)),
            reason: None,
        },
    )
    .await
    .expect_err("a clerk may not move money onto a balance either");
    assert!(refused.is_denied());

    let account = admin_order::refund_order_to_credit(
        &mut tx,
        &ctx,
        placed.id,
        RefundToCredit {
            amount: try_(dec!(40.00)),
            reason: Some("returned by hand".into()),
        },
    )
    .await
    .expect("a refund to credit");
    assert_eq!(account.customer_id, customer);
    assert_eq!(account.balance, dec!(40.00));

    let refunds: i64 = sqlx::query_scalar("select count(*) from refund where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the refund table");
    assert_eq!(refunds, 0, "no provider was asked for this money");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
