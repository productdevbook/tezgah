//! Lot tracking, from the route to the recall answer.
//!
//! `tracking_mode` could not leave `'quantity'` because nothing called
//! `set_tracking`, so FEFO, expiry and `orders_for_lot` were all unreachable.
//! The run below is the whole feature: turn tracking on, receive two lots,
//! sell one, ship it, and ask which orders the batch went out on.

mod common;

use std::sync::Arc;

use common::{Shop, Teller};
use tezgah::api::inventory_lot as route;
use tezgah::checkout::Checkout;
use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};
use tezgah::inventory::{AllocationStrategy, TrackingMode};
use tezgah::payment::PaymentProvider;
use tezgah::workflow::State;

fn in_days(days: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() + chrono::Duration::days(days)
}

#[tokio::test]
async fn a_lot_tracked_item_sells_its_earliest_date_first_and_answers_a_recall() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 0, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let tracked = route::set_tracking(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::SetTracking {
            tracking_mode: TrackingMode::Lot,
            allocation_strategy: AllocationStrategy::Fefo,
        },
    )
    .await
    .expect("the item to be counted as lots");
    assert_eq!(tracked.tracking_mode, TrackingMode::Lot);

    let later = route::receive_lot(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::ReceiveLot {
            location_id: here.location_id,
            lot_code: "LATE".into(),
            quantity: 5,
            expires_at: Some(in_days(90)),
            received_at: None,
            supplier_reference: None,
        },
    )
    .await
    .expect("a lot");

    let sooner = route::receive_lot(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::ReceiveLot {
            location_id: here.location_id,
            lot_code: "SOON".into(),
            quantity: 5,
            expires_at: Some(in_days(7)),
            received_at: None,
            supplier_reference: None,
        },
    )
    .await
    .expect("a lot");

    let lots = route::list_lots(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::ListLots::default(),
    )
    .await
    .expect("the item's lots");
    assert_eq!(lots.items.len(), 2);

    let expiring = route::list_expiring_lots(
        &mut tx,
        &ctx,
        route::ListExpiring {
            before: in_days(30),
            after: None,
            limit: None,
        },
    )
    .await
    .expect("what is going out of date");
    assert_eq!(expiring.items.len(), 1, "only one lot is near its date");
    assert_eq!(expiring.items[0].lot_code, "SOON");

    tx.commit().await.expect("to commit");

    let checkout = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id)
        .await
        .expect("a checkout");
    assert_eq!(
        placed.run.state,
        State::Done,
        "the checkout unwound: {:?}",
        placed.run.failure
    );
    let order_id = placed.order_id.expect("an order");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    // FEFO: the two that were sold came off the batch that expires first.
    let soon = route::get_lot(&mut tx, &ctx, sooner.id)
        .await
        .expect("the near-dated lot");
    let late = route::get_lot(&mut tx, &ctx, later.id)
        .await
        .expect("the far-dated lot");
    assert_eq!(soon.reserved_quantity, 2, "FEFO did not take the near one");
    assert_eq!(late.reserved_quantity, 0);

    let items = tezgah::order::items(&mut tx, &ctx, order_id, 1)
        .await
        .expect("the order's items");
    let item = items.first().expect("one item");

    fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        order_id,
        NewFulfillment {
            location_id: here.location_id,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: None,
            data: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item.id,
                inventory_item_id: Some(here.inventory_item_id),
                title: "A thing".into(),
                sku: None,
                barcode: None,
                quantity: 2,
            }],
        },
    )
    .await
    .expect("a parcel");

    let recall = route::orders_for_lot(&mut tx, &ctx, sooner.id, route::ListRecall::default())
        .await
        .expect("the recall answer");
    assert_eq!(recall.items.len(), 1, "the recall query found no parcel");
    assert_eq!(recall.items[0].order_id, Some(order_id));
    assert_eq!(recall.items[0].lot_code, "SOON");
    assert_eq!(recall.items[0].quantity, 2);

    let untouched = route::orders_for_lot(&mut tx, &ctx, later.id, route::ListRecall::default())
        .await
        .expect("the other lot's parcels");
    assert!(
        untouched.items.is_empty(),
        "a lot that never shipped answered a recall"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_lot_cannot_be_received_against_an_item_counted_as_a_number() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 5, 0).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let refused = route::receive_lot(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::ReceiveLot {
            location_id: here.location_id,
            lot_code: "A-BATCH".into(),
            quantity: 1,
            expires_at: None,
            received_at: None,
            supplier_reference: None,
        },
    )
    .await
    .expect_err("the item is counted as a number");
    assert_eq!(refused.code(), "invalid");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_breakage_comes_off_the_lot_it_happened_to() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 0, 0).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    route::set_tracking(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::SetTracking {
            tracking_mode: TrackingMode::Lot,
            allocation_strategy: AllocationStrategy::Fifo,
        },
    )
    .await
    .expect("lot tracking");

    let lot = route::receive_lot(
        &mut tx,
        &ctx,
        here.inventory_item_id,
        route::ReceiveLot {
            location_id: here.location_id,
            lot_code: "A-BATCH".into(),
            quantity: 10,
            expires_at: None,
            received_at: None,
            supplier_reference: Some("a delivery note".into()),
        },
    )
    .await
    .expect("a lot");

    let after = route::adjust_lot(
        &mut tx,
        &ctx,
        lot.id,
        route::AdjustLot {
            delta: -3,
            reason: Some("dropped".into()),
        },
    )
    .await
    .expect("the correction");
    assert_eq!(after.stocked_quantity, 7);

    let refused = route::adjust_lot(
        &mut tx,
        &ctx,
        lot.id,
        route::AdjustLot {
            delta: -100,
            reason: None,
        },
    )
    .await
    .expect_err("a lot cannot go negative");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
