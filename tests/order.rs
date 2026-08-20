//! The order, against a real Postgres.
//!
//! The four that matter: an order may not walk backwards through its statuses,
//! its total is the sum of its lines, an edit leaves the version before it
//! readable in full, and a received return puts the stock back.

mod common;

use std::sync::Arc;

use common::{Doorman, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use sqlx::Executor;
use tezgah::id::{InventoryItemId, LineItemId, StockLocationId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{
    self, ChangeAction, ChangeType, ClaimLine, ClaimRequest, ClaimType, NewAction, NewAdjustment,
    NewOrder, NewOrderLine, NewOrderShipping, NewTaxLine, OrderAddress, OrderFilter, OrderStatus,
    ReceivedLine, ReturnLine,
};
use tezgah::page::{Order as Direction, Paging, Search};
use tezgah::ports::{Actor, Ctx, Scope, Tx};
use tezgah::{inventory, page};
use uuid::Uuid;

fn drafts_only() -> OrderFilter {
    OrderFilter {
        drafts: Some(true),
        ..OrderFilter::default()
    }
}

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
            address: None,
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
    taxed.tax_lines = vec![NewTaxLine::of(dec!(18), "vat", "VAT")];
    taxed.adjustments = vec![NewAdjustment::of(dec!(5))];

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
                adjustments: Vec::new(),
                tax_lines: vec![NewTaxLine::of(dec!(18), "vat", "VAT")],
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
                images: vec!["https://example.com/claims/dent.jpg".into()],
            }],
            replacements: vec![ClaimLine {
                order_line_item_id: line_id,
                quantity: 1,
                reason: None,
                note: None,
                images: Vec::new(),
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

    // What went wrong is readable back, photo included — an operator judging
    // the claim is not taking the customer's word for it.
    let lines = order::claim_items(&mut tx, &ctx, claim.id).await?;
    let faulty = lines
        .iter()
        .find(|line| !line.is_additional_item)
        .expect("the faulty line");
    let images: Vec<String> =
        serde_json::from_value(faulty.images.clone().expect("images were written"))
            .expect("a list of urls");
    assert_eq!(images, vec!["https://example.com/claims/dent.jpg"]);
    let replacement = lines
        .iter()
        .find(|line| line.is_additional_item)
        .expect("the replacement line");
    assert!(replacement.images.is_none());

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
            cart_id: None,
            metadata: None,
        },
    )
    .await?;

    let sent = order::send_draft_for_payment(&mut tx, &ctx, draft.id, collection.id).await?;
    assert!(!sent.is_draft);
    assert_eq!(sent.status()?, OrderStatus::Pending);
    assert_eq!(sent.payment_collection_id, Some(collection.id));

    // And it is no longer in the drafts list.
    let drafts = order::list(&mut tx, &ctx, drafts_only(), Paging::first(10)).await?;
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

    let listed = order::list(&mut theirs, &ctx, OrderFilter::default(), Paging::first(10)).await?;
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

    let first = order::list(&mut tx, &ctx, OrderFilter::default(), Paging::first(2)).await?;
    assert_eq!(first.len(), 2);
    let next = first.next.as_ref().expect("another page");

    let rest = order::list(
        &mut tx,
        &ctx,
        OrderFilter::default(),
        Paging::after(page::Cursor::decode(next)?, 2),
    )
    .await?;
    assert_eq!(rest.len(), 1);
    assert!(rest.next.is_none());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// What an order is holding, and what happens to it
// ---------------------------------------------------------------------------

/// An order the way a checkout leaves one: stock reserved against a cart line,
/// and the order created carrying those reservations forward onto its own.
async fn a_held_order(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    scope: Scope,
    stocked: i32,
    quantity: i32,
) -> (
    InventoryItemId,
    StockLocationId,
    tezgah::id::OrderId,
    LineItemId,
) {
    let (item, location, variant) = a_shelf(tx, ctx, scope, stocked).await;

    // `reservation_item.cart_line_item_id` (0076) is a real, scoped foreign
    // key now: the hold has to name a cart line that exists, not a fresh id
    // standing in for one.
    let cart = Uuid::now_v7();
    sqlx::query(r#"insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(cart)
        .bind(scope.0)
        .execute(&mut **tx)
        .await
        .expect("a cart");
    let cart_line = LineItemId::new();
    sqlx::query(
        "insert into cart_line_item
             (id, scope, cart_id, product_title, quantity, unit_price, currency_code)
         values ($1, $2, $3, 'a thing', $4, 10, 'TRY')",
    )
    .bind(cart_line.as_uuid())
    .bind(scope.0)
    .bind(cart)
    .bind(quantity)
    .execute(&mut **tx)
    .await
    .expect("a cart line");

    inventory::reserve(
        tx,
        ctx,
        item,
        location,
        quantity,
        Some(cart_line),
        false,
        None,
    )
    .await
    .expect("the checkout to hold the stock");

    let mut line = a_line(quantity, dec!(10));
    line.variant_id = Some(variant);
    line.reserved_for = Some(cart_line);

    let placed = order::create(tx, ctx, an_order(vec![line]))
        .await
        .expect("an order");
    let line_id = first_line(tx, ctx, placed.id).await;

    (item, location, placed.id, line_id)
}

#[tokio::test]
async fn a_cancelled_order_gives_the_stock_it_held_back() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (item, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 3).await;

    let held = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(
        held.reserved_quantity, 3,
        "the order did not inherit a hold"
    );
    assert_eq!(held.available_quantity, 7);

    let canceled = order::cancel(&mut tx, &ctx, placed).await?;
    assert_eq!(canceled.status()?, OrderStatus::Canceled);

    let after = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(after.reserved_quantity, 0, "a cancelled order still holds");
    assert_eq!(after.stocked_quantity, 10, "nothing left the shelf");
    assert_eq!(after.available_quantity, 10);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The same shape as [`a_held_order`], with one reservation per line rather
/// than one line total — the case #159 measured at 351 queries for 50 lines.
async fn a_held_order_with_lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    scope: Scope,
    stocked: i32,
    line_quantity: i32,
    lines: usize,
) -> (InventoryItemId, StockLocationId, tezgah::id::OrderId) {
    let (item, location, variant) = a_shelf(tx, ctx, scope, stocked).await;

    let cart = Uuid::now_v7();
    sqlx::query(r#"insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(cart)
        .bind(scope.0)
        .execute(&mut **tx)
        .await
        .expect("a cart");

    let mut order_lines = Vec::with_capacity(lines);
    for _ in 0..lines {
        let cart_line = LineItemId::new();
        sqlx::query(
            "insert into cart_line_item
                 (id, scope, cart_id, product_title, quantity, unit_price, currency_code)
             values ($1, $2, $3, 'a thing', $4, 10, 'TRY')",
        )
        .bind(cart_line.as_uuid())
        .bind(scope.0)
        .bind(cart)
        .bind(line_quantity)
        .execute(&mut **tx)
        .await
        .expect("a cart line");

        inventory::reserve(
            tx,
            ctx,
            item,
            location,
            line_quantity,
            Some(cart_line),
            false,
            None,
        )
        .await
        .expect("the checkout to hold the stock");

        let mut line = a_line(line_quantity, dec!(10));
        line.variant_id = Some(variant);
        line.reserved_for = Some(cart_line);
        order_lines.push(line);
    }

    let placed = order::create(tx, ctx, an_order(order_lines))
        .await
        .expect("an order");

    (item, location, placed.id)
}

/// #159: `unwind` fanned a per-line, per-reservation loop out into 351
/// queries for a 50-line order. This does not count queries — see the pull
/// request body for why a query-counter was not worth the dependency it would
/// have taken — but it does count the one thing a batched release must not
/// lose on the way: an audit row and a `stock.released` event for every
/// reservation it gives back, not one for the order.
#[tokio::test]
async fn cancelling_a_many_line_order_gives_back_every_reservation_once() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    const LINES: usize = 20;
    let (item, location, placed) =
        a_held_order_with_lines(&mut tx, &ctx, shop.here, 1000, 1, LINES).await;

    let held = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(held.reserved_quantity, LINES as i32);

    // Reserving each line's stock already wrote its own `reservation_item`
    // audit row; only the cancellation's own rows are what this counts.
    shop.host.audits.lock().clear();
    shop.host.events.lock().clear();

    let canceled = order::cancel(&mut tx, &ctx, placed).await?;
    assert_eq!(canceled.status()?, OrderStatus::Canceled);

    let after = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(after.reserved_quantity, 0, "a cancelled order still holds");
    assert_eq!(after.stocked_quantity, 1000, "nothing left the shelf");

    let released = shop
        .host
        .audits
        .lock()
        .iter()
        .filter(|(entity, _)| *entity == "reservation_item")
        .count();
    assert_eq!(
        released, LINES,
        "one audit row per reservation released, not one per order"
    );

    let events = shop
        .host
        .events
        .lock()
        .iter()
        .filter(|name| **name == "stock.released")
        .count();
    assert_eq!(events, LINES, "one event per reservation released");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// Two connections cancelling the same order at once: the row lock
/// `hold_order` takes serializes them, so exactly one should go through and
/// the other should see a conflict rather than a partial or doubled release
/// — proven with two transactions genuinely open together, not one after the
/// other.
#[tokio::test]
async fn two_concurrent_cancellations_of_one_order_agree_on_a_winner() -> tezgah::Result<()> {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut setup, shop.here).await;
    let (item, location, placed) =
        a_held_order_with_lines(&mut setup, &ctx, shop.here, 10, 3, 1).await;
    setup.commit().await.expect("the order to stay");

    // Reserving the line's stock already wrote its own `reservation_item`
    // audit row; only the cancellation's own row is what this counts.
    shop.host.audits.lock().clear();

    let gate = Arc::new(tokio::sync::Barrier::new(2));
    let there = gate.clone();

    let first = async {
        let mut tx = shop.begin().await;
        gate.wait().await;
        let out = order::cancel(&mut tx, &shop.ctx(), placed).await;
        match &out {
            Ok(_) => tx.commit().await.expect("to keep it"),
            Err(_) => tx.rollback().await.expect("to give it back"),
        }
        out
    };
    let second = async {
        let mut tx = shop.begin().await;
        there.wait().await;
        let out = order::cancel(&mut tx, &shop.ctx(), placed).await;
        match &out {
            Ok(_) => tx.commit().await.expect("to keep it"),
            Err(_) => tx.rollback().await.expect("to give it back"),
        }
        out
    };

    let (first, second) = tokio::join!(first, second);
    let outcomes = [first, second];

    let succeeded = outcomes.iter().filter(|out| out.is_ok()).count();
    assert_eq!(
        succeeded, 1,
        "exactly one of two concurrent cancellations should win"
    );
    let losers: Vec<_> = outcomes
        .iter()
        .filter_map(|out| out.as_ref().err())
        .collect();
    assert_eq!(losers.len(), 1);
    assert!(
        losers[0].is_conflict(),
        "the loser should see a conflict, not a partial release: {}",
        losers[0].report()
    );

    let mut tx = shop.begin().await;
    let after = inventory::level(&mut tx, &shop.ctx(), item, location).await?;
    assert_eq!(after.reserved_quantity, 0, "released exactly once");
    assert_eq!(after.stocked_quantity, 10, "nothing left the shelf");

    let released = shop
        .host
        .audits
        .lock()
        .iter()
        .filter(|(entity, _)| *entity == "reservation_item")
        .count();
    assert_eq!(
        released, 1,
        "the one reservation was released exactly once, not twice"
    );

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// The cheap path has to do the whole thing too: a host that moves the status
/// is cancelling, whatever it called the call.
#[tokio::test]
async fn setting_the_status_to_cancelled_unwinds_the_same_way() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (item, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;

    order::set_status(&mut tx, &ctx, placed, OrderStatus::Canceled).await?;

    let after = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(after.reserved_quantity, 0);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cancelled_order_gives_the_promotion_use_back() -> tezgah::Result<()> {
    use tezgah::promotion::{self, NewPromotion, PromotionKind, Status};

    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let promo = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: format!("TEN-{}", Uuid::now_v7().simple()),
            kind: PromotionKind::Standard,
            status: Status::Active,
            campaign_id: None,
            is_automatic: false,
            usage_limit: None,
            customer_usage_limit: None,
            metadata: None,
        },
    )
    .await?;

    promotion::claim(&mut tx, &ctx, promo.id, None, Money::new(dec!(0), lira())).await?;
    let claimed = promotion::promotion(&mut tx, &ctx, promo.id).await?;
    assert_eq!(claimed.used, 1);

    // The order's own line carries the discount — no cart involved, and none
    // needed: `release_promotions` reads what the order itself recorded.
    let mut discounted = a_line(1, dec!(10));
    discounted.adjustments = vec![NewAdjustment {
        promotion_id: Some(promo.id.as_uuid()),
        ..NewAdjustment::of(dec!(1))
    }];

    let placed = order::create(&mut tx, &ctx, an_order(vec![discounted])).await?;

    order::cancel(&mut tx, &ctx, placed.id).await?;

    let given_back = promotion::promotion(&mut tx, &ctx, promo.id).await?;
    assert_eq!(
        given_back.used, 0,
        "a cancelled order kept the promotion's use"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_order_that_has_shipped_cannot_be_cancelled() -> tezgah::Result<()> {
    use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};

    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (_, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;
    let order_now = order::get(&mut tx, &ctx, placed).await?;
    let items = order::items(&mut tx, &ctx, placed, order_now.version).await?;
    let item = items.first().expect("an item").id;

    let parcel = fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        placed,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: None,
            data: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item,
                inventory_item_id: None,
                title: "A thing".into(),
                sku: None,
                barcode: None,
                quantity: 2,
            }],
        },
    )
    .await?;

    fulfilment::mark_packed(&mut tx, &ctx, placed, parcel.id).await?;
    fulfilment::mark_shipped(&mut tx, &ctx, placed, parcel.id, None).await?;

    let refused = order::cancel(&mut tx, &ctx, placed)
        .await
        .expect_err("a shipped order is returned, not cancelled");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_shipping_address_can_be_corrected_and_the_old_snapshot_stays_put() -> tezgah::Result<()>
{
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    let before = order::get(&mut tx, &ctx, placed.id).await?;
    let old_shipping = before.shipping_address_id.expect("an address to correct");

    let corrected = order::update_address(
        &mut tx,
        &ctx,
        placed.id,
        order::AddressKind::Shipping,
        &OrderAddress {
            address_1: Some("2 Corrected Street".into()),
            city: Some("Istanbul".into()),
            country_code: Some("TR".into()),
            ..OrderAddress::default()
        },
    )
    .await?;

    let new_shipping = corrected
        .shipping_address_id
        .expect("the order still has a shipping address");
    assert_ne!(
        new_shipping, old_shipping,
        "a correction repoints the order rather than mutating the old row"
    );
    assert_eq!(corrected.billing_address_id, before.billing_address_id);

    // The old snapshot is exactly what write_address's own comment promises:
    // frozen, not deleted, not mutated in place.
    let kept: Option<String> =
        sqlx::query_scalar("select address_1 from order_address where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(old_shipping)
            .fetch_one(&mut *tx)
            .await
            .expect("the old row to still be there");
    assert_eq!(kept.as_deref(), Some("1 Example Street"));

    assert!(
        shop.host.audited("order_address"),
        "the correction left an audit row"
    );
    let payloads = shop.host.payloads_of("order.address_updated");
    let payload = payloads.last().expect("the correction emitted an event");
    assert_eq!(payload["previous"], serde_json::json!(old_shipping));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_orders_email_can_be_corrected() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?;
    assert_eq!(placed.email.as_deref(), Some("shopper@example.com"));

    let refused = order::update_email(&mut tx, &ctx, placed.id, "not an address")
        .await
        .expect_err("a value with no @ is not an e-mail address");
    assert_eq!(refused.code(), "invalid");

    let corrected = order::update_email(&mut tx, &ctx, placed.id, "Fixed@Example.com").await?;
    assert_eq!(corrected.email.as_deref(), Some("fixed@example.com"));
    assert!(shop.host.audited("order"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn once_a_parcel_has_shipped_its_address_cannot_be_corrected_but_billing_can()
-> tezgah::Result<()> {
    use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};

    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (_, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;
    let order_now = order::get(&mut tx, &ctx, placed).await?;
    let items = order::items(&mut tx, &ctx, placed, order_now.version).await?;
    let item = items.first().expect("an item").id;

    let parcel = fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        placed,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: None,
            data: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item,
                inventory_item_id: None,
                title: "A thing".into(),
                sku: None,
                barcode: None,
                quantity: 2,
            }],
        },
    )
    .await?;

    fulfilment::mark_packed(&mut tx, &ctx, placed, parcel.id).await?;
    fulfilment::mark_shipped(&mut tx, &ctx, placed, parcel.id, None).await?;

    let new_address = OrderAddress {
        address_1: Some("Somewhere else entirely".into()),
        city: Some("Ankara".into()),
        country_code: Some("TR".into()),
        ..OrderAddress::default()
    };

    let refused = order::update_address(
        &mut tx,
        &ctx,
        placed,
        order::AddressKind::Shipping,
        &new_address,
    )
    .await
    .expect_err("the parcel is already on its way to the address on record");
    assert!(refused.is_conflict());

    // Billing carries no parcel, so it is corrected as freely as ever.
    order::update_address(
        &mut tx,
        &ctx,
        placed,
        order::AddressKind::Billing,
        &new_address,
    )
    .await?;

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_fulfilment_takes_the_stock_off_the_shelf_and_cancelling_puts_it_back()
-> tezgah::Result<()> {
    use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};

    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (stock_item, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;
    let order_now = order::get(&mut tx, &ctx, placed).await?;
    let items = order::items(&mut tx, &ctx, placed, order_now.version).await?;
    let item = items.first().expect("an item").id;

    let parcel = fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        placed,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: None,
            data: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item,
                inventory_item_id: None,
                title: "A thing".into(),
                sku: None,
                barcode: None,
                quantity: 2,
            }],
        },
    )
    .await?;

    let shipped = inventory::level(&mut tx, &ctx, stock_item, location).await?;
    assert_eq!(
        shipped.stocked_quantity, 8,
        "the goods never left the count"
    );
    assert_eq!(shipped.reserved_quantity, 0, "the hold outlived the parcel");
    assert_eq!(shipped.available_quantity, 8);

    fulfilment::cancel_fulfillment(&mut tx, &ctx, placed, parcel.id).await?;

    let back = inventory::level(&mut tx, &ctx, stock_item, location).await?;
    assert_eq!(back.stocked_quantity, 10, "the goods came back uncounted");
    assert_eq!(back.reserved_quantity, 2, "the order stopped holding them");
    assert_eq!(back.available_quantity, 8);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_edit_that_moves_a_quantity_moves_the_reservation_with_it() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (item, location, placed, line) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;

    let up = order::request_change(&mut tx, &ctx, placed, ChangeType::Edit, None).await?;
    order::add_action(
        &mut tx,
        &ctx,
        up.id,
        NewAction::on(ChangeAction::ItemUpdate, line, 5),
    )
    .await?;
    order::confirm_change(&mut tx, &ctx, up.id).await?;

    let raised = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(raised.reserved_quantity, 5, "the edit reserved nothing");
    assert_eq!(raised.available_quantity, 5);

    let down = order::request_change(&mut tx, &ctx, placed, ChangeType::Edit, None).await?;
    order::add_action(
        &mut tx,
        &ctx,
        down.id,
        NewAction::on(ChangeAction::ItemUpdate, line, 1),
    )
    .await?;
    order::confirm_change(&mut tx, &ctx, down.id).await?;

    let lowered = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(
        lowered.reserved_quantity, 1,
        "the edit held units nobody buys"
    );
    assert_eq!(lowered.available_quantity, 9);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_edit_the_shelf_cannot_meet_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (item, location, placed, line) = a_held_order(&mut tx, &ctx, shop.here, 4, 2).await;

    let change = order::request_change(&mut tx, &ctx, placed, ChangeType::Edit, None).await?;
    order::add_action(
        &mut tx,
        &ctx,
        change.id,
        NewAction::on(ChangeAction::ItemUpdate, line, 9),
    )
    .await?;

    let refused = order::confirm_change(&mut tx, &ctx, change.id)
        .await
        .expect_err("an edit past the shelf writes an order nobody can ship");
    assert!(
        refused.out_of_stock().is_some() || refused.is_conflict(),
        "an unmeetable edit came back as {}",
        refused.code()
    );

    let still = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(still.reserved_quantity, 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn more_cannot_be_received_than_the_return_asked_for() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (_, location, variant) = a_shelf(&mut tx, &ctx, shop.here, 10).await;

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
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;

    let refused = order::receive_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: 2,
            damaged: 0,
        }],
    )
    .await
    .expect_err("two came back for a return that asked for one");
    assert!(
        refused.is_conflict(),
        "over-receipt surfaced as {} rather than a conflict",
        refused.code()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_dismissal_is_checked_the_way_a_receipt_is() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let (_, location, variant) = a_shelf(&mut tx, &ctx, shop.here, 10).await;

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

    let nothing = order::dismiss_return(&mut tx, &ctx, asked.id, vec![])
        .await
        .expect_err("a dismissal of nothing");
    assert_eq!(nothing.code(), "invalid");

    let negative = order::dismiss_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: -1,
            damaged: 0,
        }],
    )
    .await
    .expect_err("a dismissal of less than nothing");
    assert_eq!(negative.code(), "invalid");

    // Nothing has come in, so there is nothing to turn away yet.
    let early = order::dismiss_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: 1,
            damaged: 0,
        }],
    )
    .await
    .expect_err("a dismissal of what was never received");
    assert!(early.is_conflict());

    order::receive_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: 2,
            damaged: 0,
        }],
    )
    .await?;

    let dismissed = order::dismiss_return(
        &mut tx,
        &ctx,
        asked.id,
        vec![ReceivedLine {
            order_line_item_id: line_id,
            quantity: 1,
            damaged: 0,
        }],
    )
    .await?;
    assert_eq!(dismissed.id, asked.id);

    let now = order::get(&mut tx, &ctx, placed.id).await?;
    let items = order::items(&mut tx, &ctx, placed.id, now.version).await?;
    assert_eq!(
        items.first().map(|row| row.return_dismissed_quantity),
        Some(1)
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_cannot_cancel_this_one_s_order() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;
    let (item, location, placed, _) = a_held_order(&mut tx, &ctx, shop.here, 10, 2).await;
    tx.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let refused = order::cancel(&mut theirs, &shop.theirs(), placed)
        .await
        .expect_err("another scope reached this order");
    assert!(refused.is_not_found());
    theirs.rollback().await.expect("to roll back");

    let mut tx = shop.begin().await;
    let still = inventory::level(&mut tx, &ctx, item, location).await?;
    assert_eq!(still.reserved_quantity, 2, "somebody else let the stock go");
    tx.rollback().await.expect("to roll back");

    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// What a line was discounted by, and taxed at
// ---------------------------------------------------------------------------

/// The discount and the rates are rows of their own, in the order's own
/// tables — not two strings in a jsonb column nothing can constrain.
#[tokio::test]
async fn a_line_keeps_its_discount_and_its_rates_in_tables() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let mut line = a_line(2, dec!(100));
    line.adjustments = vec![NewAdjustment {
        code: Some("SUMMER".into()),
        description: Some("Summer".into()),
        ..NewAdjustment::of(dec!(10))
    }];
    line.tax_lines = vec![NewTaxLine::of(dec!(18), "vat", "VAT")];

    let placed = order::create(&mut tx, &ctx, an_order(vec![line])).await?;
    let line_id = first_line(&mut tx, &ctx, placed.id).await;

    let (amount, code, currency): (Decimal, Option<String>, String) = sqlx::query_as(
        "select amount, code, currency_code from order_line_item_adjustment
         where scope = $1 and order_line_item_id = $2",
    )
    .bind(shop.here.0)
    .bind(line_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("an adjustment row");

    assert_eq!(amount, dec!(10));
    assert_eq!(code.as_deref(), Some("SUMMER"));
    assert_eq!(currency, "TRY");

    let (rate, tax_code): (Decimal, String) = sqlx::query_as(
        "select rate, code from order_line_item_tax_line
         where scope = $1 and order_line_item_id = $2",
    )
    .bind(shop.here.0)
    .bind(line_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("a tax line");

    assert_eq!(rate, dec!(18));
    assert_eq!(tax_code, "vat");

    let empty: Option<String> = sqlx::query_scalar(
        "select i.metadata::text from order_item i
         where i.scope = $1 and i.order_id = $2",
    )
    .bind(shop.here.0)
    .bind(placed.id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the item");
    assert!(
        empty.is_none(),
        "the item still carries charges in metadata"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// Two rates against one line stay two rows, and the total is what both of
/// them come to together.
#[tokio::test]
async fn two_stacked_rates_are_two_rows_and_one_total() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let mut line = a_line(2, dec!(100));
    line.adjustments = vec![NewAdjustment::of(dec!(10))];
    line.tax_lines = vec![
        NewTaxLine::of(dec!(18), "vat", "VAT"),
        NewTaxLine::of(dec!(8), "city", "City levy"),
    ];

    let placed = order::create(&mut tx, &ctx, an_order(vec![line])).await?;
    let line_id = first_line(&mut tx, &ctx, placed.id).await;

    let rates: Vec<Decimal> = sqlx::query_scalar(
        "select rate from order_line_item_tax_line
         where scope = $1 and order_line_item_id = $2
         order by rate",
    )
    .bind(shop.here.0)
    .bind(line_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the tax lines");

    assert_eq!(rates, vec![dec!(8), dec!(18)], "the rates were blended");

    let totals = order::totals(&mut tx, &ctx, placed.id, placed.version).await?;
    assert_eq!(totals.subtotal.amount, dec!(200));
    assert_eq!(totals.discount.amount, dec!(10));
    assert_eq!(totals.tax.amount, dec!(49.40));
    assert_eq!(totals.total.amount, dec!(239.40));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// A tax-inclusive line carries its tax inside the price, so the rows have to
/// be read back net or the order is worth more than the shopper was shown.
#[tokio::test]
async fn a_tax_inclusive_line_is_added_up_net() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let mut line = a_line(1, dec!(118));
    line.is_tax_inclusive = true;
    line.tax_lines = vec![NewTaxLine::of(dec!(18), "vat", "VAT")];

    let placed = order::create(&mut tx, &ctx, an_order(vec![line])).await?;
    let totals = order::totals(&mut tx, &ctx, placed.id, placed.version).await?;

    assert_eq!(totals.subtotal.amount, dec!(100));
    assert_eq!(totals.tax.amount, dec!(18));
    assert_eq!(totals.total.amount, dec!(118));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// Shipping keeps its own rows, and carries them into the next version.
#[tokio::test]
async fn shipping_keeps_its_rates_across_a_version() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            shipping: vec![NewOrderShipping {
                name: "Courier".into(),
                description: None,
                shipping_option_id: None,
                amount: money(dec!(100)),
                is_tax_inclusive: false,
                data: None,
                adjustments: vec![NewAdjustment::of(dec!(20))],
                tax_lines: vec![NewTaxLine::of(dec!(10), "vat", "VAT")],
            }],
            ..an_order(vec![a_line(1, dec!(10))])
        },
    )
    .await?;

    let before = order::totals(&mut tx, &ctx, placed.id, placed.version).await?;
    assert_eq!(before.shipping.amount, dec!(100));
    assert_eq!(before.discount.amount, dec!(20));
    // A hundred less the twenty off, taxed at ten per cent.
    assert_eq!(before.tax.amount, dec!(8));

    let change = order::request_change(
        &mut tx,
        &ctx,
        placed.id,
        ChangeType::Edit,
        Some("a second look".into()),
    )
    .await?;
    order::confirm_change(&mut tx, &ctx, change.id).await?;

    let after = order::totals(&mut tx, &ctx, placed.id, placed.version + 1).await?;
    assert_eq!(after, before, "the new version lost the shipping's charges");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The rows belong to the scope that wrote them and to nothing else.
#[tokio::test]
async fn another_scope_sees_none_of_a_line_s_charges() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let mut line = a_line(1, dec!(100));
    line.adjustments = vec![NewAdjustment::of(dec!(10))];
    line.tax_lines = vec![NewTaxLine::of(dec!(18), "vat", "VAT")];
    let placed = order::create(&mut tx, &ctx, an_order(vec![line])).await?;
    tx.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let adjustments: i64 = sqlx::query_scalar("select count(*) from order_line_item_adjustment")
        .fetch_one(&mut *theirs)
        .await
        .expect("a count");
    let taxes: i64 = sqlx::query_scalar("select count(*) from order_line_item_tax_line")
        .fetch_one(&mut *theirs)
        .await
        .expect("a count");

    assert_eq!(adjustments, 0, "another scope read a discount");
    assert_eq!(taxes, 0, "another scope read a tax rate");

    let refused = order::totals(&mut theirs, &shop.theirs(), placed.id, placed.version)
        .await
        .expect_err("another scope added up this order");
    assert!(refused.is_not_found());

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The instalment sale as the order sees it: the ledger settles against what
/// the card was charged, and the price tag is still there to invoice for.
#[tokio::test]
async fn an_instalment_sale_reconciles_against_what_the_card_was_charged() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(2, dec!(500))])).await?;
    let totals = order::totals(&mut tx, &ctx, placed.id, placed.version).await?;
    assert_eq!(totals.total.amount, dec!(1000.00));

    let collection = tezgah::payment::create_collection(
        &mut tx,
        &ctx,
        tezgah::payment::NewCollection {
            amount: totals.total,
            cart_id: None,
            metadata: None,
        },
    )
    .await?;
    sqlx::query(
        "update payment_collection set surcharge_amount = $3, charged_amount = $4,
                installment_count = 3
         where scope = $1 and id = $2",
    )
    .bind(shop.here.0)
    .bind(collection.id.as_uuid())
    .bind(dec!(90.00))
    .bind(dec!(1090.00))
    .execute(&mut *tx)
    .await
    .expect("the plan the bank accepted");

    sqlx::query(r#"update "order" set payment_collection_id = $3 where scope = $1 and id = $2"#)
        .bind(shop.here.0)
        .bind(placed.id.as_uuid())
        .bind(collection.id.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the collection to be the order's");

    let owing = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(owing.surcharge.amount, dec!(90.00));
    assert_eq!(owing.charged.amount, dec!(1090.00));
    assert_eq!(owing.due.amount, dec!(1090.00));

    order::record_transaction(
        &mut tx,
        &ctx,
        placed.id,
        money(dec!(1090.00)),
        "capture",
        Uuid::now_v7(),
    )
    .await?;

    let settled = order::ledger(&mut tx, &ctx, placed.id).await?;
    assert_eq!(
        settled.due.amount,
        dec!(0),
        "a correctly paid instalment sale read as overpaid"
    );
    assert_eq!(settled.state, order::PaymentState::Captured);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

fn an_invoice(number: &str, external: &str, total: Decimal) -> order::NewInvoice {
    order::NewInvoice {
        number: number.to_owned(),
        external_id: Some(external.to_owned()),
        provider: Some("an-integrator".into()),
        status: order::InvoiceStatus::Issued,
        total: money(total),
        issued_at: None,
        document_url: None,
        metadata: None,
    }
}

#[tokio::test]
async fn one_invoice_cannot_be_recorded_against_an_order_twice() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;

    let first = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000001",
            "1b0f4a6e-0000-4000-8000-000000000001",
            dec!(100.00),
        ),
    )
    .await?;
    assert_eq!(first.kind(), order::InvoiceKind::Invoice);
    assert_eq!(first.status()?, order::InvoiceStatus::Issued);

    let again = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000001",
            "1b0f4a6e-0000-4000-8000-000000000002",
            dec!(100.00),
        ),
    )
    .await;
    assert!(
        again.is_err(),
        "the same serial went on the same order twice, which is two invoices for one sale"
    );

    let elsewhere = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000002",
            "1b0f4a6e-0000-4000-8000-000000000001",
            dec!(100.00),
        ),
    )
    .await;
    assert!(
        elsewhere.is_err(),
        "the authority's own identifier landed twice"
    );

    assert_eq!(order::invoices(&mut tx, &ctx, placed.id).await?.len(), 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The rule is about the sale, not about the document. An integrator asked
/// twice allocates a fresh serial and a fresh identifier, so keying uniqueness
/// on either refuses nothing.
#[tokio::test]
async fn an_order_carries_one_invoice_whatever_serial_it_arrives_under() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;

    order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000100",
            "1b0f4a6e-0000-4000-8000-000000000100",
            dec!(100.00),
        ),
    )
    .await?;

    let fresh_pair = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "XYZ2026000000999",
            "1b0f4a6e-0000-4000-8000-000000000999",
            dec!(100.00),
        ),
    )
    .await;
    assert!(
        fresh_pair.is_err(),
        "a second serial and a second identifier are still a second invoice for one sale"
    );

    // The stage a retry actually happens at: the authority has not answered,
    // so there is no identifier for the old partial index to key on.
    let retried = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        order::NewInvoice {
            number: "XYZ2026000001000".into(),
            external_id: None,
            provider: Some("an-integrator".into()),
            status: order::InvoiceStatus::Requested,
            total: money(dec!(100.00)),
            issued_at: None,
            document_url: None,
            metadata: None,
        },
    )
    .await;
    assert!(
        retried.is_err(),
        "asking again before the authority answered opened a second invoice"
    );

    assert_eq!(order::invoices(&mut tx, &ctx, placed.id).await?.len(), 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// Correcting an invoice that stands is a credit note; a cancelled one leaves
/// the sale with no document, so its replacement has to be admitted.
#[tokio::test]
async fn a_cancelled_invoice_can_be_reissued() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;

    let first = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000200",
            "1b0f4a6e-0000-4000-8000-000000000200",
            dec!(100.00),
        ),
    )
    .await?;

    order::set_invoice_status(&mut tx, &ctx, first.id, order::InvoiceStatus::Cancelled).await?;

    let reissued = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000201",
            "1b0f4a6e-0000-4000-8000-000000000201",
            dec!(100.00),
        ),
    )
    .await?;
    assert_eq!(reissued.status()?, order::InvoiceStatus::Issued);

    assert_eq!(order::invoices(&mut tx, &ctx, placed.id).await?.len(), 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_credit_note_names_the_invoice_it_reverses() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    let placed = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;
    let other = order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;

    let issued = order::record_invoice(
        &mut tx,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000010",
            "1b0f4a6e-0000-4000-8000-000000000010",
            dec!(100.00),
        ),
    )
    .await?;

    let note = order::record_credit_note(
        &mut tx,
        &ctx,
        placed.id,
        issued.id,
        an_invoice(
            "IAD2026000000001",
            "1b0f4a6e-0000-4000-8000-000000000011",
            dec!(40.00),
        ),
    )
    .await?;

    assert_eq!(note.kind(), order::InvoiceKind::CreditNote);
    assert_eq!(note.replaces_invoice_id, Some(issued.id));

    let strayed = order::record_credit_note(
        &mut tx,
        &ctx,
        other.id,
        issued.id,
        an_invoice(
            "IAD2026000000002",
            "1b0f4a6e-0000-4000-8000-000000000012",
            dec!(40.00),
        ),
    )
    .await;
    assert!(
        strayed.is_err(),
        "a credit note reversed a document belonging to another order"
    );

    // A credit note has nothing to reverse but an invoice.
    let doubled = order::record_credit_note(
        &mut tx,
        &ctx,
        placed.id,
        note.id,
        an_invoice(
            "IAD2026000000003",
            "1b0f4a6e-0000-4000-8000-000000000013",
            dec!(10.00),
        ),
    )
    .await;
    assert!(doubled.is_err());

    let accepted =
        order::set_invoice_status(&mut tx, &ctx, issued.id, order::InvoiceStatus::Accepted).await?;
    assert_eq!(accepted.status()?, order::InvoiceStatus::Accepted);

    assert_eq!(order::invoices(&mut tx, &ctx, placed.id).await?.len(), 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_shop_cannot_see_an_invoice() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut mine = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut mine, shop.here).await;

    let placed = order::create(&mut mine, &ctx, an_order(vec![a_line(1, dec!(100))])).await?;
    let issued = order::record_invoice(
        &mut mine,
        &ctx,
        placed.id,
        an_invoice(
            "ABC2026000000020",
            "1b0f4a6e-0000-4000-8000-000000000020",
            dec!(100.00),
        ),
    )
    .await?;
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let seen = order::invoice(&mut theirs, &shop.theirs(), issued.id).await;
    assert!(seen.is_err(), "another shop read an invoice of this one's");
    theirs.rollback().await.expect("to roll back");

    shop.close().await;
    Ok(())
}

/// 0022 gave every `order_item` write an AFTER trigger that updates the parent
/// `"order"` row, so the parent is a lock every child write takes. A return
/// restocked before writing items and a cancellation wrote items before
/// releasing stock: the same two rows in opposite orders. Two connections, at
/// the same time, because a deadlock is not visible one call after another.
#[tokio::test]
async fn a_return_and_a_cancellation_on_one_order_do_not_deadlock() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut setup = shop.begin().await;
    seed_currency(&mut setup, shop.here).await;
    let (_, location, variant) = a_shelf(&mut setup, &ctx, shop.here, 1000).await;
    setup.commit().await.expect("the shelf to stay");

    for round in 0..12 {
        let mut tx = shop.begin().await;
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
        tx.commit().await.expect("the order to stay");

        let gate = Arc::new(tokio::sync::Barrier::new(2));
        let there = gate.clone();

        let returning = async {
            let mut tx = shop.begin().await;
            gate.wait().await;
            let out = order::receive_return(
                &mut tx,
                &ctx,
                asked.id,
                vec![ReceivedLine {
                    order_line_item_id: line_id,
                    quantity: 2,
                    damaged: 0,
                }],
            )
            .await;
            let _ = tx.rollback().await;
            out.err()
        };

        let cancelling = async {
            let mut tx = shop.begin().await;
            there.wait().await;
            let out = order::cancel(&mut tx, &ctx, placed.id).await;
            let _ = tx.rollback().await;
            out.err()
        };

        let (received, cancelled) = tokio::join!(returning, cancelling);

        for refused in [received, cancelled].into_iter().flatten() {
            assert_ne!(
                refused.sqlstate().as_deref(),
                Some("40P01"),
                "round {round}: postgres killed one of them for a deadlock: {}",
                refused.report()
            );
        }
    }

    shop.close().await;
    Ok(())
}

/// `display_id` was `max(display_id) + 1`, which two connections read as the
/// same number. Two at once, not one after the other: a simulated race proves
/// nothing.
#[tokio::test]
async fn two_checkouts_at_once_are_given_two_different_numbers() {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    seed_currency(&mut setup, shop.here).await;
    setup.commit().await.expect("to keep the currency");

    let place = || async {
        let mut tx = shop.begin().await;
        let placed = order::create(&mut tx, &shop.ctx(), an_order(vec![a_line(1, dec!(10))])).await;

        match placed {
            Ok(order) => {
                tx.commit().await.expect("to keep the order");
                Ok(order.display_id)
            }
            Err(err) => {
                tx.rollback().await.expect("to give it back");
                Err(err)
            }
        }
    };

    let (first, second) = tokio::join!(place(), place());
    let first = first.expect("the first checkout to go through");
    let second = second.expect("the second checkout to go through");

    assert!(
        first.is_some() && second.is_some(),
        "an order with no number"
    );
    assert_ne!(
        first, second,
        "two checkouts were given the same order number"
    );

    shop.close().await;
}

/// A shop that already had orders keeps counting from where it was: an order
/// number is what a shop reconciles its books against, and restarting at one
/// would put two sales under the same number.
#[tokio::test]
async fn the_counter_carries_on_above_the_numbers_a_shop_already_has() {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    seed_currency(&mut setup, shop.here).await;

    // The database as 0036 finds it: numbered rows and no counter.
    sqlx::query("delete from display_counter where scope = $1")
        .bind(shop.here.0)
        .execute(&mut *setup)
        .await
        .expect("to take the counter away");
    sqlx::query(
        r#"insert into "order" (id, scope, display_id, currency_code) values ($1, $2, 500, 'TRY')"#,
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *setup)
    .await
    .expect("an order from before");
    setup.commit().await.expect("to keep it");

    let owner = shop.migrator().await;
    owner
        .execute(include_str!(
            "../migrations/0036_display_counter_and_released_holds.sql"
        ))
        .await
        .expect("0036 to apply to a database that has orders in it");
    owner.close().await;

    let mut tx = shop.begin().await;
    let placed = order::create(&mut tx, &shop.ctx(), an_order(vec![a_line(1, dec!(10))]))
        .await
        .expect("an order");
    tx.commit().await.expect("to commit");

    assert_eq!(
        placed.display_id,
        Some(501),
        "the counter started again under numbers the shop already has"
    );

    shop.close().await;
}

/// There is no domain writer for `return_reason` — the admin API inserts it
/// directly — so a test needing one does the same.
async fn a_return_reason(tx: &mut Tx<'_>, scope: Scope, label: &str) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("insert into return_reason (id, scope, value, label) values ($1, $2, $3, $4)")
        .bind(id)
        .bind(scope.0)
        .bind(id.simple().to_string())
        .bind(label)
        .execute(&mut **tx)
        .await
        .expect("a return reason");
    id
}

/// The list a customer picks from when sending something back, in one
/// language: falls back to the shop's own label until a translation exists.
#[tokio::test]
async fn a_return_reasons_label_is_localised_and_falls_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let reason = a_return_reason(&mut tx, shop.here, "Wrong size").await;

    let fallen_back = order::localised_return_reason(&mut tx, &ctx, reason, "tr")
        .await
        .expect("a reading");
    assert!(fallen_back.is_fallback);
    assert_eq!(fallen_back.label, "Wrong size");

    order::put_return_reason_translation(
        &mut tx,
        &ctx,
        reason,
        order::ReturnReasonTranslation {
            return_reason_id: reason,
            locale: "tr".into(),
            label: "Yanlış beden".into(),
            description: None,
        },
    )
    .await
    .expect("a translation");

    let read = order::localised_return_reason(&mut tx, &ctx, reason, "tr")
        .await
        .expect("a reading");
    assert!(!read.is_fallback);
    assert_eq!(read.label, "Yanlış beden");

    assert_eq!(
        order::return_reason_translations(&mut tx, &ctx, reason)
            .await
            .expect("its translations")
            .len(),
        1
    );

    order::remove_return_reason_translation(&mut tx, &ctx, reason, "tr")
        .await
        .expect("to remove it");
    assert!(
        order::localised_return_reason(&mut tx, &ctx, reason, "tr")
            .await
            .expect("a reading")
            .is_fallback
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Nothing but another tenant's reason id, so `tezgah_fk`'s composite key is
/// what refuses this.
#[tokio::test]
async fn a_return_reason_translation_cannot_point_at_another_scopes_reason() {
    let shop = Shop::open().await;

    let mut theirs_tx = shop.begin_as(shop.elsewhere).await;
    let theirs = a_return_reason(&mut theirs_tx, shop.elsewhere, "Their reason").await;
    theirs_tx.commit().await.expect("to commit");

    let mut mine = shop.begin().await;
    let refused = sqlx::query(
        "insert into return_reason_translation (id, scope, return_reason_id, locale, label)
         values ($1, $2, $3, 'tr', 'Yanlış beden')",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(theirs)
    .execute(&mut *mine)
    .await;
    mine.rollback().await.expect("to give the connection back");

    assert!(
        refused.is_err(),
        "a translation in one scope pointed at another scope's return reason"
    );

    shop.close().await;
}

/// `return_reason` has no soft delete, so this exercises the constraint
/// against a real hard delete rather than one the application never does.
#[tokio::test]
async fn a_deleted_return_reason_takes_its_translations_with_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let reason = a_return_reason(&mut tx, shop.here, "Wrong size").await;

    order::put_return_reason_translation(
        &mut tx,
        &ctx,
        reason,
        order::ReturnReasonTranslation {
            return_reason_id: reason,
            locale: "tr".into(),
            label: "Yanlış beden".into(),
            description: None,
        },
    )
    .await
    .expect("a translation");

    sqlx::query("delete from return_reason where id = $1")
        .bind(reason)
        .execute(&mut *tx)
        .await
        .expect("to delete the reason");

    let left: i64 = sqlx::query_scalar(
        "select count(*) from return_reason_translation where return_reason_id = $1",
    )
    .bind(reason)
    .fetch_one(&mut *tx)
    .await
    .expect("to count");
    assert_eq!(left, 0, "the translation outlived the reason it named");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// An operator looking for one order out of forty thousand has an e-mail or a
/// number off a receipt, and until now had neither to search with.
#[tokio::test]
async fn an_order_is_found_by_its_email_or_its_number() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let mine = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            email: Some("ada@example.com".into()),
            ..an_order(vec![a_line(1, dec!(10))])
        },
    )
    .await?;

    order::create(
        &mut tx,
        &ctx,
        NewOrder {
            email: Some("grace@example.com".into()),
            ..an_order(vec![a_line(1, dec!(10))])
        },
    )
    .await?;

    let searching = |text: &str| OrderFilter {
        search: Search::new(text),
        ..OrderFilter::default()
    };

    let by_email = order::list(&mut tx, &ctx, searching("ADA@"), Paging::first(10)).await?;
    assert_eq!(by_email.len(), 1, "the e-mail matches, case and all");
    assert_eq!(by_email.items[0].id, mine.id);

    let number = mine.display_id.expect("an order has a display number");
    let by_number = order::list(
        &mut tx,
        &ctx,
        searching(&number.to_string()),
        Paging::first(10),
    )
    .await?;
    assert!(
        by_number.items.iter().any(|row| row.id == mine.id),
        "the display number matches too"
    );

    let nobody = order::list(&mut tx, &ctx, searching("nobody"), Paging::first(10)).await?;
    assert!(nobody.is_empty());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// A back office opening Orders wants today's, not the first order the shop
/// ever took — and paging through newest-first has to keep working, which is
/// the half that is easy to get wrong.
#[tokio::test]
async fn newest_first_pages_the_other_way_and_still_ends() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let mut made = Vec::new();
    for _ in 0..3 {
        made.push(order::create(&mut tx, &ctx, an_order(vec![a_line(1, dec!(10))])).await?);
    }

    let newest = OrderFilter {
        order: Direction::Newest,
        ..OrderFilter::default()
    };

    let first = order::list(&mut tx, &ctx, newest.clone(), Paging::first(2)).await?;
    assert_eq!(first.len(), 2);
    assert_eq!(
        first.items[0].id, made[2].id,
        "the newest order is on page one"
    );
    assert_eq!(first.items[1].id, made[1].id);

    let next = first.next.as_ref().expect("another page");
    let rest = order::list(
        &mut tx,
        &ctx,
        newest,
        Paging::after(page::Cursor::decode(next)?, 2),
    )
    .await?;
    assert_eq!(rest.len(), 1, "the cursor walked backwards in time, once");
    assert_eq!(rest.items[0].id, made[0].id);
    assert!(rest.next.is_none());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
