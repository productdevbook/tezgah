//! The order, against a real Postgres.
//!
//! The four that matter: an order may not walk backwards through its statuses,
//! its total is the sum of its lines, an edit leaves the version before it
//! readable in full, and a received return puts the stock back.

mod common;

use common::{Doorman, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::id::{InventoryItemId, LineItemId, StockLocationId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{
    self, ChangeAction, ChangeType, ClaimLine, ClaimRequest, ClaimType, NewAction, NewOrder,
    NewOrderLine, NewOrderShipping, OrderAddress, OrderStatus, ReceivedLine, ReturnLine,
};
use tezgah::page::Paging;
use tezgah::ports::{Actor, Ctx, Scope, Tx};
use tezgah::{inventory, page};
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

async fn a_variant(tx: &mut Tx<'_>, scope: Scope) -> VariantId {
    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(scope.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut **tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(scope.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut **tx)
    .await
    .expect("a variant");

    variant
}

/// A stock location holding `stocked` of one item, and the variant that
/// consumes one of it.
async fn a_shelf(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    scope: Scope,
    stocked: i32,
) -> (InventoryItemId, StockLocationId, VariantId) {
    let location = inventory::create_stock_location(
        tx,
        ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        tx,
        ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(tx, ctx, item.id, location.id, stocked, 0)
        .await
        .expect("a level");

    let variant = a_variant(tx, scope).await;
    inventory::attach_inventory_item(tx, ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    (item.id, location.id, variant)
}

fn an_order(lines: Vec<NewOrderLine>) -> NewOrder {
    NewOrder {
        email: Some("shopper@example.com".into()),
        shipping_address: Some(OrderAddress {
            address_1: Some("1 Example Street".into()),
            city: Some("Istanbul".into()),
            country_code: Some("TR".into()),
            ..OrderAddress::default()
        }),
        lines,
        ..NewOrder::of(lira())
    }
}

fn a_line(quantity: i32, price: Decimal) -> NewOrderLine {
    NewOrderLine::of("A thing", quantity, money(price))
}

async fn first_line(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order: tezgah::id::OrderId) -> LineItemId {
    order::line_items(tx, ctx, order)
        .await
        .expect("its lines")
        .first()
        .expect("a line")
        .id
}

#[tokio::test]
async fn an_order_cannot_walk_backwards() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    assert_eq!(placed.status()?, OrderStatus::Pending);

    let done = order::set_status(&mut tx, &ctx, placed.id, OrderStatus::Completed).await?;
    assert_eq!(done.status()?, OrderStatus::Completed);

    let refused = order::set_status(&mut tx, &ctx, placed.id, OrderStatus::Pending)
        .await
        .expect_err("a completed order does not go back to pending");
    assert!(refused.is_conflict());

    order::set_status(&mut tx, &ctx, placed.id, OrderStatus::Archived).await?;
    let refused = order::set_status(&mut tx, &ctx, placed.id, OrderStatus::Completed)
        .await
        .expect_err("an archived order is finished");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_order_totals_what_its_lines_total() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let mut taxed = a_line(3, dec!(19.99));
    taxed.tax_rate = dec!(18);
    taxed.discount = dec!(5);

    let placed = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            shipping: vec![NewOrderShipping {
                name: "Courier".into(),
                description: None,
                shipping_option_id: None,
                amount: money(dec!(12.50)),
                is_tax_inclusive: false,
                data: None,
                discount: Decimal::ZERO,
                tax_rate: dec!(18),
            }],
            ..an_order(vec![taxed, a_line(2, dec!(0.05))])
        },
    )
    .await?;

    let totals = order::totals(&mut tx, &ctx, placed.id, placed.version).await?;
    assert_eq!(
        totals.total.amount,
        totals.subtotal.amount - totals.discount.amount
            + totals.shipping.amount
            + totals.tax.amount
    );

    let items = order::items(&mut tx, &ctx, placed.id, placed.version).await?;
    let lines = order::line_items(&mut tx, &ctx, placed.id).await?;
    let gross: Decimal = items
        .iter()
        .map(|item| {
            let price = item.unit_price.unwrap_or_default();
            price * Decimal::from(item.quantity)
        })
        .sum();
    assert_eq!(gross, dec!(19.99) * dec!(3) + dec!(0.05) * dec!(2));
    assert_eq!(items.len(), lines.len());

    // The summary written beside version 1 says the same thing the sum does.
    let summary = order::summary(&mut tx, &ctx, placed.id, placed.version).await?;
    assert_eq!(
        summary.totals["total"].as_str(),
        Some(totals.total.amount.to_string().as_str())
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_confirmed_change_leaves_the_old_version_readable() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;
    let line = first_line(&mut tx, &ctx, placed.id).await;

    let change = order::request_change(&mut tx, &ctx, placed.id, ChangeType::Edit, None).await?;
    order::add_action(
        &mut tx,
        &ctx,
        change.id,
        NewAction::on(ChangeAction::ItemUpdate, line, 5),
    )
    .await?;

    let edited = order::confirm_change(&mut tx, &ctx, change.id).await?;
    assert_eq!(edited.version, placed.version + 1);

    let before = order::items(&mut tx, &ctx, placed.id, placed.version).await?;
    let after = order::items(&mut tx, &ctx, placed.id, edited.version).await?;
    assert_eq!(before.first().map(|item| item.quantity), Some(2));
    assert_eq!(after.first().map(|item| item.quantity), Some(5));

    // And the old summary is still there beside the old rows.
    let old = order::summary(&mut tx, &ctx, placed.id, placed.version).await?;
    let new = order::summary(&mut tx, &ctx, placed.id, edited.version).await?;
    assert_eq!(old.totals["total"].as_str(), Some("20.00"));
    assert_eq!(new.totals["total"].as_str(), Some("50.00"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_declined_change_moves_nothing() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;
    let line = first_line(&mut tx, &ctx, placed.id).await;

    let change = order::request_change(&mut tx, &ctx, placed.id, ChangeType::Edit, None).await?;
    order::add_action(
        &mut tx,
        &ctx,
        change.id,
        NewAction::on(ChangeAction::ItemUpdate, line, 5),
    )
    .await?;
    order::decline_change(&mut tx, &ctx, change.id, Some("no".into())).await?;

    let still = order::get(&mut tx, &ctx, placed.id).await?;
    assert_eq!(still.version, placed.version);

    let refused = order::confirm_change(&mut tx, &ctx, change.id)
        .await
        .expect_err("a declined change cannot then be confirmed");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_received_return_puts_the_stock_back() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (item, location, variant) = a_shelf(&mut tx, &ctx, shop.here, 10).await;

    let mut line = a_line(3, dec!(10));
    line.variant_id = Some(variant);
    let placed = order::create(&mut tx, &ctx, an_order(vec![line])).await?;
    let line_id = first_line(&mut tx, &ctx, placed.id).await;

    let asked = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        Some(location),
        vec![ReturnLine {
            order_line_item_id: line_id,
            quantity: 2,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    let order_now = order::get(&mut tx, &ctx, placed.id).await?;
    let items = order::items(&mut tx, &ctx, placed.id, order_now.version).await?;
    assert_eq!(
        items.first().map(|row| row.return_requested_quantity),
        Some(2)
    );

    let received = order::receive_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: 2,
            damaged: 1,
        }],
    )
    .await?;
    assert_eq!(received.status, "received");

    let order_now = order::get(&mut tx, &ctx, placed.id).await?;
    let items = order::items(&mut tx, &ctx, placed.id, order_now.version).await?;
    assert_eq!(
        items.first().map(|row| row.return_received_quantity),
        Some(2)
    );

    // Ten on the shelf, one sellable back, and the damaged one not.
    let level = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(level.stocked_quantity, 11);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_claim_replaces_through_the_change_mechanism() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;
    let line_id = first_line(&mut tx, &ctx, placed.id).await;

    let claim = order::request_claim(
        &mut tx,
        &ctx,
        placed.id,
        ClaimRequest {
            claim_type: ClaimType::Replace,
            faulty: vec![ClaimLine {
                order_line_item_id: line_id,
                quantity: 1,
                reason: Some("production_failure".into()),
                note: None,
            }],
            replacements: vec![ClaimLine {
                order_line_item_id: line_id,
                quantity: 1,
                reason: None,
                note: None,
            }],
            collect: false,
            location_id: None,
            refund_amount: None,
        },
    )
    .await?;
    assert_eq!(claim.claim_type, "replace");

    let now = order::get(&mut tx, &ctx, placed.id).await?;
    assert_eq!(now.version, placed.version + 1);

    let items = order::items(&mut tx, &ctx, placed.id, now.version).await?;
    let item = items.first().expect("the line at the new version");
    assert_eq!(item.written_off_quantity, 1);
    assert_eq!(item.quantity, 3);

    // The claim went through the one mechanism rather than a second one.
    let changes = order::changes(&mut tx, &ctx, placed.id, Paging::first(10)).await?;
    assert!(changes.items.iter().any(|change| {
        change.change_type == "claim" && change.order_claim_id == Some(claim.id)
    }));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_exchange_writes_both_halves_or_neither() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;
    let line_id = first_line(&mut tx, &ctx, placed.id).await;

    let half = order::request_exchange(
        &mut tx,
        &ctx,
        placed.id,
        order::ExchangeRequest {
            returning: vec![ReturnLine {
                order_line_item_id: line_id,
                quantity: 1,
                return_reason_id: None,
                note: None,
            }],
            outbound: vec![],
            location_id: None,
            allow_backorder: false,
            difference_due: None,
        },
    )
    .await;
    assert!(half.is_err(), "an exchange with one half is refused");

    let both = order::request_exchange(
        &mut tx,
        &ctx,
        placed.id,
        order::ExchangeRequest {
            returning: vec![ReturnLine {
                order_line_item_id: line_id,
                quantity: 1,
                return_reason_id: None,
                note: None,
            }],
            outbound: vec![order::ExchangeLine {
                order_line_item_id: line_id,
                quantity: 1,
                note: None,
            }],
            location_id: None,
            allow_backorder: false,
            difference_due: Some(money(dec!(0))),
        },
    )
    .await?;

    let now = order::get(&mut tx, &ctx, placed.id).await?;
    let items = order::items(&mut tx, &ctx, placed.id, now.version).await?;
    let item = items.first().expect("the line at the new version");
    assert_eq!(item.return_requested_quantity, 1);
    assert_eq!(item.quantity, 3);
    assert_eq!(both.order_id, placed.id);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_draft_is_priced_like_any_other_order() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let draft = order::create_draft(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;
    assert!(draft.is_draft);
    assert_eq!(draft.status()?, OrderStatus::Draft);

    let totals = order::totals(&mut tx, &ctx, draft.id, draft.version).await?;
    assert_eq!(totals.total.amount, dec!(20.00));

    let collection = tezgah::payment::create_collection(
        &mut tx,
        &ctx,
        tezgah::payment::NewCollection {
            amount: totals.total,
            metadata: None,
        },
    )
    .await?;

    let sent = order::send_draft_for_payment(&mut tx, &ctx, draft.id, collection.id).await?;
    assert!(!sent.is_draft);
    assert_eq!(sent.status()?, OrderStatus::Pending);
    assert_eq!(sent.payment_collection_id, Some(collection.id));

    // And it is no longer in the drafts list.
    let drafts = order::list(&mut tx, &ctx, None, Some(true), Paging::first(10)).await?;
    assert!(drafts.is_empty());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn the_ledger_says_what_is_paid_rather_than_a_column() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(10))])).await?;

    let empty = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(empty.state, order::PaymentState::NotPaid);
    assert_eq!(empty.due.amount, dec!(20.00));

    order::record_transaction(
        &mut tx,
        &ctx,
        placed.id,
        money(dec!(20)),
        "capture",
        Uuid::now_v7(),
    )
    .await?;

    let paid = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(paid.state, order::PaymentState::Captured);
    assert_eq!(paid.due.amount, Decimal::ZERO);

    order::record_transaction(
        &mut tx,
        &ctx,
        placed.id,
        money(dec!(-5)),
        "refund",
        Uuid::now_v7(),
    )
    .await?;

    let back = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(back.state, order::PaymentState::PartiallyRefunded);
    assert_eq!(back.paid.amount, dec!(15));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn the_same_capture_cannot_be_written_twice() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    let capture = Uuid::now_v7();

    order::record_transaction(
        &mut tx,
        &ctx,
        placed.id,
        money(dec!(10)),
        "capture",
        capture,
    )
    .await?;
    let again = order::record_transaction(
        &mut tx,
        &ctx,
        placed.id,
        money(dec!(10)),
        "capture",
        capture,
    )
    .await
    .expect_err("the ledger refuses the same movement twice");
    assert!(again.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_sees_no_orders() -> tezgah::Result<()> {
    let shop = Shop::open().await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    tx.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let ctx = shop.theirs();

    let unseen = order::get(&mut theirs, &ctx, placed.id)
        .await
        .expect_err("somebody else's order is not there");
    assert!(unseen.is_not_found());

    let listed = order::list(&mut theirs, &ctx, None, None, Paging::first(10)).await?;
    assert!(listed.is_empty());

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn nothing_is_read_without_being_allowed_to() -> tezgah::Result<()> {
    let shop = Shop::open().await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;
    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    tx.commit().await.expect("to commit");

    let doorman = Doorman;
    let refused = shop.ctx_as(Actor::System, &doorman);
    let mut tx = shop.begin().await;

    let denied = order::get(&mut tx, &refused, placed.id)
        .await
        .expect_err("a refused actor reads nothing");
    assert!(denied.is_denied());

    let denied = order::totals(&mut tx, &refused, placed.id, 1)
        .await
        .expect_err("nor totals");
    assert!(denied.is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn one_order_at_a_time_may_be_being_changed() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    order::request_change(&mut tx, &ctx, placed.id, ChangeType::Edit, None).await?;

    let second = order::request_change(&mut tx, &ctx, placed.id, ChangeType::Edit, None).await;
    assert!(second.is_err(), "two open changes on one order is refused");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_page_of_orders_carries_a_cursor() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    for _ in 0..3 {
        order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    }

    let first = order::list(&mut tx, &ctx, None, None, Paging::first(2)).await?;
    assert_eq!(first.len(), 2);
    let next = first.next.as_ref().expect("another page");

    let rest = order::list(
        &mut tx,
        &ctx,
        None,
        None,
        Paging::after(page::Cursor::decode(next)?, 2),
    )
    .await?;
    assert_eq!(rest.len(), 1);
    assert!(rest.next.is_none());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
