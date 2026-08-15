//! What the buyer accepted, and when the right to walk away runs out.
//!
//! A distance seller has to prove both, for three years, against a text that
//! may have been rewritten since. So the questions worth asking are whether
//! the accepted text survives the template being replaced, whether the clock
//! starts at delivery rather than at the order, and whether a line that was
//! exempt on the day it was sold stays exempt.

mod common;

use chrono::{Duration, SubsecRound, Utc};
use common::{Recorder, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};
use tezgah::id::{OrderId, StockLocationId};
use tezgah::money::{Currency, Money};
use tezgah::order::{
    self, Acceptance, AgreementKind, NewAgreement, NewOrder, NewOrderLine, WithdrawalExclusion,
};
use tezgah::page::Paging;
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
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

async fn a_location(tx: &mut Tx<'_>, scope: Scope) -> StockLocationId {
    let location = StockLocationId::new();
    sqlx::query("insert into stock_location (id, scope, name) values ($1, $2, 'Depot')")
        .bind(location.as_uuid())
        .bind(scope.0)
        .execute(&mut **tx)
        .await
        .expect("a location");
    location
}

fn an_order(line: NewOrderLine) -> NewOrder {
    NewOrder {
        email: Some("shopper@example.com".into()),
        lines: vec![line],
        ..NewOrder::of(lira())
    }
}

fn a_form(body: &str) -> NewAgreement {
    NewAgreement {
        kind: AgreementKind::PreContract,
        locale: "tr".into(),
        body: body.into(),
        effective_from: None,
        metadata: None,
    }
}

/// Packs, ships and delivers everything on the order, on a stopped clock.
async fn deliver(
    tx: &mut Tx<'_>,
    shop: &Shop,
    order: OrderId,
    location: StockLocationId,
    at: chrono::DateTime<Utc>,
) {
    let ctx = shop.ctx();
    let current = order::get(tx, &ctx, order).await.expect("the order");
    let items = order::items(tx, &ctx, order, current.version)
        .await
        .expect("its items");
    let item = items.first().expect("an item");

    let parcel = fulfilment::create_fulfillment(
        tx,
        &ctx,
        order,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: None,
            data: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item.id,
                inventory_item_id: None,
                title: "A thing".into(),
                sku: None,
                barcode: None,
                quantity: item.quantity,
            }],
        },
    )
    .await
    .expect("a fulfilment");

    let clock = Recorder::at(at);
    let then = Ctx::new(shop.here, Actor::System, clock.as_ref() as &dyn Host);

    fulfilment::mark_packed(tx, &then, order, parcel.id)
        .await
        .expect("packing");
    fulfilment::mark_shipped(tx, &then, order, parcel.id, None)
        .await
        .expect("shipping");
    fulfilment::mark_delivered(tx, &then, order, parcel.id)
        .await
        .expect("delivery");
}

#[tokio::test]
async fn the_text_a_buyer_accepted_outlives_the_one_published_after_it() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let first = order::publish_agreement(&mut tx, &ctx, a_form("The terms of 2025")).await?;
    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;
    order::accept_agreement(&mut tx, &ctx, placed.id, Acceptance::of(first.id)).await?;

    // The shop rewrites its form. Every order placed before this one still has
    // to be answerable with the words its buyer read.
    let second = order::publish_agreement(&mut tx, &ctx, a_form("The terms of 2026")).await?;
    assert_ne!(first.id, second.id);

    let read = order::accepted_text(&mut tx, &ctx, placed.id, AgreementKind::PreContract).await?;
    assert_eq!(read.body, "The terms of 2025");
    assert_eq!(read.id, first.id);
    assert_eq!(read.body_hash, first.body_hash);

    let published = order::agreement_versions(&mut tx, &ctx, None, Paging::first(10)).await?;
    assert_eq!(published.len(), 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_published_agreement_cannot_be_rewritten() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let version = order::publish_agreement(&mut tx, &ctx, a_form("As it was written")).await?;
    tx.commit().await.expect("to commit");

    let mut editing = shop.begin().await;
    let rewritten = sqlx::query(
        "update agreement_version set body = 'Something else' where scope = $1 and id = $2",
    )
    .bind(shop.here.0)
    .bind(version.id.as_uuid())
    .execute(&mut *editing)
    .await;
    assert!(
        rewritten.is_err(),
        "the text a buyer accepted was editable in place"
    );
    editing.rollback().await.expect("to roll back");

    let mut deleting = shop.begin().await;
    let removed = sqlx::query("delete from agreement_version where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(version.id.as_uuid())
        .execute(&mut *deleting)
        .await;
    assert!(removed.is_err(), "evidence was deletable");
    deleting.rollback().await.expect("to roll back");

    // Nothing about it moved.
    let mut reading = shop.begin().await;
    let still = order::agreement_version(&mut reading, &ctx, version.id).await?;
    assert_eq!(still.body, "As it was written");
    reading.rollback().await.expect("to roll back");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn one_order_accepts_one_document_of_a_kind() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let version = order::publish_agreement(&mut tx, &ctx, a_form("The form")).await?;
    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;

    let accepted = order::accept_agreement(
        &mut tx,
        &ctx,
        placed.id,
        Acceptance {
            ip: Some("198.51.100.7".into()),
            user_agent: Some("a browser".into()),
            ..Acceptance::of(version.id)
        },
    )
    .await?;
    assert_eq!(accepted.kind, "pre_contract");
    assert_eq!(accepted.ip.as_deref(), Some("198.51.100.7"));

    let again = order::accept_agreement(&mut tx, &ctx, placed.id, Acceptance::of(version.id))
        .await
        .expect_err("a second acceptance of the same kind");
    assert!(again.is_conflict());

    let kept = order::agreements(&mut tx, &ctx, placed.id).await?;
    assert_eq!(kept.len(), 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn the_withdrawal_window_runs_from_delivery_and_not_from_the_order() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;

    // Nothing has been handed over, so there is no deadline to state. Not an
    // expired one, and not one counted from the order.
    let before = order::withdrawal_deadline(&mut tx, &ctx, placed.id).await?;
    assert_eq!(before.len(), 1);
    assert!(before[0].eligible);
    assert!(before[0].delivered_at.is_none());
    assert!(before[0].deadline.is_none());

    let delivered = (Utc::now() + Duration::days(30)).trunc_subsecs(0);
    deliver(&mut tx, &shop, placed.id, location, delivered).await;

    let after = order::withdrawal_deadline(&mut tx, &ctx, placed.id).await?;
    let window = &after[0];
    let deadline = window.deadline.expect("a deadline once delivered");
    assert_eq!(deadline, delivered + Duration::days(14));
    assert!(
        deadline > placed.created_at + Duration::days(14),
        "the clock was started by the order rather than by the delivery"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_made_to_order_line_is_marked_exempt_at_the_sale_and_stays_exempt() -> tezgah::Result<()>
{
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine {
            selling_plan_id: None,
            withdrawal_exclusion: Some(WithdrawalExclusion::CustomMade),
            ..NewOrderLine::of("A name engraved on it", 1, money(dec!(10)))
        }),
    )
    .await?;

    let lines = order::line_items(&mut tx, &ctx, placed.id).await?;
    assert!(!lines[0].withdrawal_eligible);
    assert_eq!(
        lines[0].withdrawal_exclusion_reason.as_deref(),
        Some("custom_made")
    );

    deliver(
        &mut tx,
        &shop,
        placed.id,
        location,
        Utc::now().trunc_subsecs(0),
    )
    .await;

    // Delivering it does not open a window that never existed.
    let after = order::withdrawal_deadline(&mut tx, &ctx, placed.id).await?;
    assert!(!after[0].eligible);
    assert!(after[0].delivered_at.is_some());
    assert!(after[0].deadline.is_none());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn withdrawing_names_the_day_the_money_is_owed_by() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;
    let line = order::line_items(&mut tx, &ctx, placed.id).await?[0].id;

    deliver(
        &mut tx,
        &shop,
        placed.id,
        location,
        Utc::now().trunc_subsecs(0),
    )
    .await;

    let opened = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        Some(location),
        vec![order::ReturnLine {
            order_line_item_id: line,
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    let notice = Utc::now().trunc_subsecs(0);
    let clock = Recorder::at(notice);
    let then = Ctx::new(shop.here, Actor::System, clock.as_ref() as &dyn Host);

    let notified = order::notify_withdrawal(&mut tx, &then, opened.id).await?;
    assert_eq!(notified.notified_at, Some(notice));
    assert_eq!(notified.refund_due_by, Some(notice + Duration::days(14)));

    let again = order::notify_withdrawal(&mut tx, &then, opened.id)
        .await
        .expect_err("a second notice");
    assert!(again.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_withdrawal_before_any_delivery_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;
    let line = order::line_items(&mut tx, &ctx, placed.id).await?[0].id;

    let opened = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        Some(location),
        vec![order::ReturnLine {
            order_line_item_id: line,
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    // Nothing handed over, so no window has opened to be inside of.
    let early = order::notify_withdrawal(&mut tx, &ctx, opened.id)
        .await
        .expect_err("a withdrawal before any delivery");
    assert!(early.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_withdrawal_on_the_fifteenth_day_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;
    let line = order::line_items(&mut tx, &ctx, placed.id).await?[0].id;

    let delivered = Utc::now().trunc_subsecs(0);
    deliver(&mut tx, &shop, placed.id, location, delivered).await;

    let opened = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        Some(location),
        vec![order::ReturnLine {
            order_line_item_id: line,
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    let clock = Recorder::at(delivered + Duration::days(15));
    let too_late = Ctx::new(shop.here, Actor::System, clock.as_ref() as &dyn Host);
    let refused = order::notify_withdrawal(&mut tx, &too_late, opened.id)
        .await
        .expect_err("a withdrawal on the fifteenth day");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_exempt_line_cannot_be_withdrawn_from() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let location = a_location(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine {
            selling_plan_id: None,
            withdrawal_exclusion: Some(WithdrawalExclusion::Hygiene),
            ..NewOrderLine::of("Something opened", 1, money(dec!(10)))
        }),
    )
    .await?;
    let line = order::line_items(&mut tx, &ctx, placed.id).await?[0].id;

    deliver(
        &mut tx,
        &shop,
        placed.id,
        location,
        Utc::now().trunc_subsecs(0),
    )
    .await;

    let opened = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        Some(location),
        vec![order::ReturnLine {
            order_line_item_id: line,
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    let refused = order::notify_withdrawal(&mut tx, &ctx, opened.id)
        .await
        .expect_err("a withdrawal from a line outside the right");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_sees_neither_the_text_nor_the_acceptance() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let version = order::publish_agreement(&mut tx, &ctx, a_form("Ours")).await?;
    let placed = order::create(
        &mut tx,
        &ctx,
        an_order(NewOrderLine::of("A thing", 1, money(dec!(10)))),
    )
    .await?;
    order::accept_agreement(&mut tx, &ctx, placed.id, Acceptance::of(version.id)).await?;
    tx.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let other = shop.theirs();

    let hidden = order::agreement_version(&mut theirs, &other, version.id)
        .await
        .expect_err("another shop reading our form");
    assert!(hidden.is_not_found());

    let unreachable = order::agreements(&mut theirs, &other, placed.id)
        .await
        .expect_err("another shop reading our acceptances");
    assert!(unreachable.is_not_found());

    let none = order::agreement_versions(&mut theirs, &other, None, Paging::first(10)).await?;
    assert!(none.is_empty());

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
