//! Handing an order to somebody else.
//!
//! Ownership moves and nothing else does: no version is written, no item is
//! copied. What is worth asking is who may move it, what the token is worth
//! after it expires, and whether another scope can see the offer at all.

mod common;

use chrono::{Duration, Utc};
use common::{Recorder, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::id::CustomerId;
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};
use uuid::Uuid;

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: Decimal) -> Money {
    Money::new(amount, lira())
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

fn an_order(owner: CustomerId) -> NewOrder {
    NewOrder {
        customer_id: Some(owner),
        email: Some("shopper@example.com".into()),
        lines: vec![NewOrderLine::of("A thing", 1, money(dec!(10)))],
        ..NewOrder::of(lira())
    }
}

fn tomorrow() -> chrono::DateTime<Utc> {
    Utc::now() + Duration::days(1)
}

#[tokio::test]
async fn accepting_a_transfer_moves_the_order_to_the_new_customer() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let owner = CustomerId::new();
    let taker = CustomerId::new();
    let placed = order::create(&mut tx, &ctx, an_order(owner)).await?;

    let offered = order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "next@example.com".into(),
        tomorrow(),
    )
    .await?;
    assert_eq!(offered.transfer.status, "requested");
    assert_eq!(offered.transfer.from_customer_id, Some(owner));
    assert!(!offered.token.is_empty());

    let moved = order::accept_transfer(&mut tx, &ctx, placed.id, &offered.token, taker).await?;
    assert_eq!(moved.customer_id, Some(taker));

    assert!(shop.host.emitted("order.transferred"));
    assert!(shop.host.audited("order_transfer"));

    // The claim is spent: the same token does not move it a second time.
    let again = order::accept_transfer(&mut tx, &ctx, placed.id, &offered.token, taker)
        .await
        .expect_err("a settled transfer cannot be accepted again");
    assert!(again.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_wrong_token_moves_nothing() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let owner = CustomerId::new();
    let placed = order::create(&mut tx, &ctx, an_order(owner)).await?;
    let offered = order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "next@example.com".into(),
        tomorrow(),
    )
    .await?;

    let mut wrong = offered.token.clone();
    wrong.push('0');
    let refused = order::accept_transfer(&mut tx, &ctx, placed.id, &wrong, CustomerId::new())
        .await
        .expect_err("a token that was never issued");
    assert!(refused.is_denied());

    let still = order::get(&mut tx, &ctx, placed.id).await?;
    assert_eq!(still.customer_id, Some(owner));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn declining_leaves_the_order_where_it_was() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let owner = CustomerId::new();
    let placed = order::create(&mut tx, &ctx, an_order(owner)).await?;
    let offered = order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "next@example.com".into(),
        tomorrow(),
    )
    .await?;

    let declined = order::decline_transfer(&mut tx, &ctx, placed.id, &offered.token).await?;
    assert_eq!(declined.status, "declined");
    assert!(declined.settled_at.is_some());

    let still = order::get(&mut tx, &ctx, placed.id).await?;
    assert_eq!(still.customer_id, Some(owner));

    // Declined and gone: the order is free to be offered again.
    order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "somebody@example.com".into(),
        tomorrow(),
    )
    .await?;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn cancelling_withdraws_the_offer() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let owner = CustomerId::new();
    let placed = order::create(&mut tx, &ctx, an_order(owner)).await?;
    let offered = order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "next@example.com".into(),
        tomorrow(),
    )
    .await?;

    let canceled = order::cancel_transfer(&mut tx, &ctx, placed.id).await?;
    assert_eq!(canceled.status, "canceled");

    let refused =
        order::accept_transfer(&mut tx, &ctx, placed.id, &offered.token, CustomerId::new())
            .await
            .expect_err("a withdrawn offer is not there to take");
    assert!(refused.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_order_is_offered_to_one_person_at_a_time() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(CustomerId::new())).await?;
    order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "one@example.com".into(),
        tomorrow(),
    )
    .await?;

    let refused = order::request_transfer(
        &mut tx,
        &ctx,
        placed.id,
        "two@example.com".into(),
        tomorrow(),
    )
    .await
    .expect_err("one open offer at a time");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_transfer_that_has_expired_cannot_be_accepted() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(CustomerId::new())).await?;
    let expires = Utc::now() + Duration::hours(1);
    let offered =
        order::request_transfer(&mut tx, &ctx, placed.id, "late@example.com".into(), expires)
            .await?;

    // A clock stopped after the offer ran out, rather than a test that waits.
    let later = Recorder::at(expires + Duration::hours(1));
    let then = Ctx::new(shop.here, Actor::System, later.as_ref() as &dyn Host);

    let refused =
        order::accept_transfer(&mut tx, &then, placed.id, &offered.token, CustomerId::new())
            .await
            .expect_err("an expired transfer");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_transfer_of_somebody_elses_order_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(CustomerId::new())).await?;

    let stranger = common::Doorman;
    let theirs = Ctx::new(
        shop.here,
        Actor::Customer { id: Uuid::now_v7() },
        &stranger as &dyn Host,
    );

    let refused = order::request_transfer(
        &mut tx,
        &theirs,
        placed.id,
        "thief@example.com".into(),
        tomorrow(),
    )
    .await
    .expect_err("an order that is not theirs");
    assert!(refused.is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_cannot_see_or_take_a_transfer() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut mine = shop.begin().await;
    seed_currency(&mut mine, shop.here).await;
    let placed = order::create(&mut mine, &ctx, an_order(CustomerId::new())).await?;
    let offered = order::request_transfer(
        &mut mine,
        &ctx,
        placed.id,
        "next@example.com".into(),
        tomorrow(),
    )
    .await?;
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let refused = order::accept_transfer(
        &mut theirs,
        &shop.theirs(),
        placed.id,
        &offered.token,
        CustomerId::new(),
    )
    .await
    .expect_err("another shop's transfer");
    assert!(refused.is_not_found());

    let seen: i64 = sqlx::query_scalar("select count(*) from order_transfer")
        .fetch_one(&mut *theirs)
        .await
        .expect("to count");
    assert_eq!(seen, 0);

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
