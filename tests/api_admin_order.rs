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
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')",
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

/// The collection a payment belongs to, made the order's own, which is how a
/// capture finds the ledger it has to move.
async fn pay_for(
    tx: &mut Tx<'_>,
    scope: Scope,
    order_id: OrderId,
    payment: PaymentId,
) -> PaymentCollectionId {
    let collection: Uuid = sqlx::query_scalar(
        "select payment_collection_id from payment where scope = $1 and id = $2",
    )
    .bind(scope.0)
    .bind(payment.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the collection");

    sqlx::query(r#"update "order" set payment_collection_id = $3 where scope = $1 and id = $2"#)
        .bind(scope.0)
        .bind(order_id.as_uuid())
        .bind(collection)
        .execute(&mut **tx)
        .await
        .expect("the order to be paying through it");

    PaymentCollectionId::from_uuid(collection)
}

#[tokio::test]
async fn a_capture_reaches_the_orders_ledger_and_a_refund_takes_it_back() -> Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let placed = admin_order::create_order(&mut tx, &ctx, an_order(dec!(100.00))).await?;
    let held = a_held_payment(&mut tx, &ctx, money(dec!(100.00))).await;
    pay_for(&mut tx, shop.here, placed.id, held).await;

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
    let collection = pay_for(&mut tx, shop.here, placed.id, held).await;

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
    let collection = pay_for(&mut tx, shop.here, placed.id, held).await;
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
