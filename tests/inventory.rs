//! Stock, against a real Postgres.
//!
//! The one that matters is `two_carts_race_for_the_last_unit`: two connections
//! reaching for the same unit at the same moment, on two transactions that are
//! genuinely open together. Doing one after the other would pass whatever the
//! code did.

mod common;

use common::Shop;
use tezgah::fulfilment;
use tezgah::id::{InventoryItemId, OrderId, OrderItemId, StockLocationId, VariantId};
use tezgah::inventory;
use tezgah::page::Paging;
use tezgah::ports::{Ctx, Scope, Tx};

async fn second_location(shop: &Shop) -> StockLocationId {
    let mut tx = shop.begin().await;
    let location = inventory::create_stock_location(
        &mut tx,
        &shop.ctx(),
        inventory::NewStockLocation {
            name: format!("warehouse {}", uuid::Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a second location");
    tx.commit().await.expect("to commit");
    location.id
}

/// A location, an item, a level holding `stocked`, and a variant that consumes
/// one of the item.
async fn seed(shop: &Shop, stocked: i32) -> (InventoryItemId, StockLocationId, VariantId) {
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", uuid::Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", uuid::Uuid::now_v7())),
            title: Some("a thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, stocked, 0)
        .await
        .expect("a level");

    let variant = variant(&mut tx, shop.here).await;
    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    tx.commit().await.expect("to commit the seed");

    (item.id, location.id, variant)
}

/// The catalogue has no module yet, so its rows are written here by hand.
async fn variant(tx: &mut Tx<'_>, scope: Scope) -> VariantId {
    let product = uuid::Uuid::now_v7();
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

async fn stock(shop: &Shop, item: InventoryItemId, location: StockLocationId) -> (i32, i32, i32) {
    let mut tx = shop.begin().await;
    let level = inventory::level(&mut tx, &shop.ctx(), item, location)
        .await
        .expect("a level");
    tx.commit().await.expect("to commit");
    (
        level.stocked_quantity,
        level.reserved_quantity,
        level.available_quantity,
    )
}

#[tokio::test]
async fn reserving_promises_stock_without_moving_it() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 5).await;

    let mut tx = shop.begin().await;
    inventory::reserve(&mut tx, &shop.ctx(), item, location, 2, None, false, None)
        .await
        .expect("two of five");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (5, 2, 3));
    assert!(shop.host.emitted("stock.reserved"));

    shop.close().await;
}

#[tokio::test]
async fn reserving_more_than_is_available_is_refused() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 2).await;

    let mut tx = shop.begin().await;
    let err = inventory::reserve(&mut tx, &shop.ctx(), item, location, 3, None, false, None)
        .await
        .expect_err("more than there is");
    tx.commit().await.expect("to commit");

    assert_eq!(err.out_of_stock(), Some(item.as_uuid()));
    assert_eq!(stock(&shop, item, location).await, (2, 0, 2));

    shop.close().await;
}

/// The claim this whole module exists for.
#[tokio::test]
async fn two_carts_race_for_the_last_unit() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 1).await;

    let one = shop.begin().await;
    let two = shop.begin().await;

    async fn take(
        mut tx: Tx<'static>,
        ctx: Ctx<'_>,
        item: InventoryItemId,
        location: StockLocationId,
    ) -> tezgah::Result<()> {
        let outcome = inventory::reserve(&mut tx, &ctx, item, location, 1, None, false, None).await;
        match outcome {
            Ok(_) => {
                tx.commit().await.map_err(tezgah::Error::from)?;
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.map_err(tezgah::Error::from)?;
                Err(err)
            }
        }
    }

    let (first, second) = tokio::join!(
        take(one, shop.ctx(), item, location),
        take(two, shop.ctx(), item, location),
    );

    let losers: Vec<_> = [&first, &second]
        .into_iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();

    assert_eq!(
        losers.len(),
        1,
        "exactly one of the two should have got the unit"
    );
    assert_eq!(losers[0].out_of_stock(), Some(item.as_uuid()));
    assert_eq!(stock(&shop, item, location).await, (1, 1, 0));

    let mut tx = shop.begin().await;
    let held = inventory::reservations(&mut tx, &shop.ctx(), Paging::first(10))
        .await
        .expect("the reservations");
    tx.commit().await.expect("to commit");
    assert_eq!(held.len(), 1, "the loser left a reservation behind");

    shop.close().await;
}

#[tokio::test]
async fn fulfilling_drops_the_reservation_and_the_stock() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 4).await;

    let mut tx = shop.begin().await;
    let reservation =
        inventory::reserve(&mut tx, &shop.ctx(), item, location, 3, None, false, None)
            .await
            .expect("three of four");
    inventory::fulfil(&mut tx, &shop.ctx(), reservation.id)
        .await
        .expect("to ship them");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (1, 0, 1));

    let mut tx = shop.begin().await;
    let held = inventory::reservations(&mut tx, &shop.ctx(), Paging::first(10))
        .await
        .expect("the reservations");
    tx.commit().await.expect("to commit");
    assert!(held.is_empty());

    shop.close().await;
}

#[tokio::test]
async fn releasing_gives_the_promise_back_and_leaves_the_shelf_alone() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 4).await;

    let mut tx = shop.begin().await;
    let reservation =
        inventory::reserve(&mut tx, &shop.ctx(), item, location, 3, None, false, None)
            .await
            .expect("three of four");
    inventory::release(&mut tx, &shop.ctx(), reservation.id)
        .await
        .expect("to give them back");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (4, 0, 4));
    assert!(shop.host.emitted("stock.released"));

    shop.close().await;
}

#[tokio::test]
async fn a_backorder_may_promise_more_than_is_held() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 1).await;

    let mut tx = shop.begin().await;
    let reservation = inventory::reserve(&mut tx, &shop.ctx(), item, location, 3, None, true, None)
        .await
        .expect("a backorder");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (1, 3, -2));

    let mut tx = shop.begin().await;
    let err = inventory::fulfil(&mut tx, &shop.ctx(), reservation.id)
        .await
        .expect_err("what is not on the shelf cannot ship");
    assert!(err.is_conflict());
    tx.rollback().await.expect("to roll back");

    shop.close().await;
}

#[tokio::test]
async fn a_reservation_that_ran_out_of_time_comes_back() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 5).await;

    let then = chrono::Utc::now() - chrono::Duration::minutes(30);

    let mut tx = shop.begin().await;
    inventory::reserve(
        &mut tx,
        &shop.ctx(),
        item,
        location,
        2,
        None,
        false,
        Some(then),
    )
    .await
    .expect("a held cart");
    inventory::reserve(&mut tx, &shop.ctx(), item, location, 1, None, false, None)
        .await
        .expect("one that never expires");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (5, 3, 2));

    let mut tx = shop.begin().await;
    let freed = inventory::expire_reservations(&mut tx, &shop.ctx(), chrono::Utc::now())
        .await
        .expect("to sweep");
    tx.commit().await.expect("to commit");

    assert_eq!(freed, 1);
    assert_eq!(stock(&shop, item, location).await, (5, 1, 4));
    assert!(shop.host.emitted("stock.released"));

    shop.close().await;
}

#[tokio::test]
async fn adjusting_moves_the_shelf_and_is_written_down() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 5).await;

    let mut tx = shop.begin().await;
    let level = inventory::adjust_stock(&mut tx, &shop.ctx(), item, location, -2, Some("breakage"))
        .await
        .expect("two broken");
    assert_eq!(level.stocked_quantity, 3);

    let err = inventory::adjust_stock(&mut tx, &shop.ctx(), item, location, -9, None)
        .await
        .expect_err("below none is not a count");
    assert!(err.is_conflict());
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (3, 0, 3));
    assert!(shop.host.audited("inventory_level"));

    shop.close().await;
}

#[tokio::test]
async fn a_bundle_can_only_be_sold_as_often_as_its_scarcest_part() {
    let shop = Shop::open().await;
    let (item, location, variant) = seed(&shop, 10).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let scarce = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", uuid::Uuid::now_v7())),
            ..inventory::NewInventoryItem::default()
        },
    )
    .await
    .expect("a second item");

    inventory::set_stock(&mut tx, &ctx, scarce.id, location, 5, 0)
        .await
        .expect("five of it");
    inventory::attach_inventory_item(&mut tx, &ctx, variant, scarce.id, 2)
        .await
        .expect("two of it per variant");

    let available = inventory::availability_for_variant(&mut tx, &ctx, variant, None)
        .await
        .expect("an availability");
    tx.commit().await.expect("to commit");

    assert_eq!(available, 2, "ten of one and five of a pair is two bundles");

    let mut tx = shop.begin().await;
    inventory::reserve(&mut tx, &shop.ctx(), item, location, 10, None, false, None)
        .await
        .expect("all of the first");
    let available = inventory::availability_for_variant(&mut tx, &shop.ctx(), variant, None)
        .await
        .expect("an availability");
    tx.commit().await.expect("to commit");

    assert_eq!(available, 0);

    shop.close().await;
}

#[tokio::test]
async fn another_scope_sees_none_of_it() {
    let shop = Shop::open().await;
    let (item, location, variant) = seed(&shop, 5).await;

    let mut tx = shop.begin_as(shop.elsewhere).await;
    let theirs = shop.theirs();

    assert!(
        inventory::level(&mut tx, &theirs, item, location)
            .await
            .expect_err("not theirs")
            .is_not_found()
    );

    assert!(
        inventory::stock_locations(&mut tx, &theirs, Paging::first(10))
            .await
            .expect("a page")
            .is_empty()
    );

    assert!(
        inventory::reserve(&mut tx, &theirs, item, location, 1, None, false, None)
            .await
            .expect_err("not theirs")
            .is_not_found()
    );

    assert_eq!(
        inventory::availability_for_variant(&mut tx, &theirs, variant, None)
            .await
            .expect("an availability"),
        0
    );
    tx.rollback().await.expect("to roll back");

    assert_eq!(stock(&shop, item, location).await, (5, 0, 5));

    shop.close().await;
}

/// A count that disagrees with what was already promised is believed, and said
/// out loud: the goods are owed and nobody asked for a backorder.
#[tokio::test]
async fn counting_less_than_was_promised_says_so() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 10).await;

    let mut tx = shop.begin().await;
    inventory::reserve(&mut tx, &shop.ctx(), item, location, 8, None, false, None)
        .await
        .expect("eight of ten");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    inventory::adjust_stock(
        &mut tx,
        &shop.ctx(),
        item,
        location,
        -7,
        Some("counted the shelf"),
    )
    .await
    .expect("a count is believed");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (3, 8, -5));
    assert!(
        shop.host.emitted("stock.oversold"),
        "five units are owed that do not exist and nothing said so"
    );

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Lots, serials and expiry.
// ---------------------------------------------------------------------------

/// A location, a lot-tracked item and a variant that consumes one of it. No
/// level is set: a lot-tracked item gets its count by receiving lots.
async fn lot_seed(
    shop: &Shop,
    strategy: inventory::AllocationStrategy,
    mode: inventory::TrackingMode,
) -> (InventoryItemId, StockLocationId, VariantId) {
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", uuid::Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", uuid::Uuid::now_v7())),
            title: Some("a perishable thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_tracking(&mut tx, &ctx, item.id, mode, strategy)
        .await
        .expect("to turn tracking on");

    let variant = variant(&mut tx, shop.here).await;
    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    tx.commit().await.expect("to commit the seed");

    (item.id, location.id, variant)
}

fn days(count: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::Utc::now() + chrono::Duration::days(count)
}

/// Which lot code holds a reservation, and how much of it.
async fn claimed(shop: &Shop, item: InventoryItemId) -> Vec<(String, i32)> {
    let mut tx = shop.begin().await;
    let rows: Vec<(String, i32)> = sqlx::query_as(
        "select l.lot_code, rl.quantity
         from reservation_lot rl
         join inventory_lot l on l.scope = rl.scope and l.id = rl.inventory_lot_id
         where rl.scope = $1 and l.inventory_item_id = $2
         order by l.lot_code",
    )
    .bind(shop.here.0)
    .bind(item.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the lot claims");
    tx.commit().await.expect("to commit");
    rows
}

/// Nothing about an item that does not track lots changed: it is stocked by a
/// number, reserved against that number, and leaves no lot rows behind.
#[tokio::test]
async fn an_item_that_does_not_track_lots_is_counted_exactly_as_before() {
    let shop = Shop::open().await;
    let (item, location, _) = seed(&shop, 5).await;

    let mut tx = shop.begin().await;
    let held = inventory::reserve(&mut tx, &shop.ctx(), item, location, 2, None, false, None)
        .await
        .expect("two of five");
    inventory::fulfil(&mut tx, &shop.ctx(), held.id)
        .await
        .expect("to ship them");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, location).await, (3, 0, 3));

    let mut tx = shop.begin().await;
    let lots: i64 = sqlx::query_scalar(
        "select count(*) from inventory_lot where scope = $1 and inventory_item_id = $2",
    )
    .bind(shop.here.0)
    .bind(item.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("a count");
    let claims: i64 = sqlx::query_scalar("select count(*) from reservation_lot where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("a count");
    tx.commit().await.expect("to commit");

    assert_eq!(
        (lots, claims),
        (0, 0),
        "lot tracking leaked into an item that asked for none"
    );

    shop.close().await;
}

/// The oldest date leaves first, whatever order the goods arrived in.
#[tokio::test]
async fn the_lot_that_expires_first_is_the_lot_that_leaves() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    // Received first, out of date last: FIFO would take this one.
    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "keeps".into(),
            expires_at: Some(days(60)),
            received_at: Some(days(-30)),
            quantity: 1,
            supplier_reference: None,
        },
    )
    .await
    .expect("the first delivery");

    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "soon".into(),
            expires_at: Some(days(3)),
            received_at: Some(days(-1)),
            quantity: 1,
            supplier_reference: None,
        },
    )
    .await
    .expect("the second delivery");

    inventory::reserve(&mut tx, &ctx, item, location, 1, None, false, None)
        .await
        .expect("one of two");
    tx.commit().await.expect("to commit");

    assert_eq!(claimed(&shop, item).await, vec![("soon".to_string(), 1)]);
    // The level is still the sum of the lots, and one of the two is promised.
    assert_eq!(stock(&shop, item, location).await, (2, 1, 1));

    shop.close().await;
}

/// The same two lots under FIFO: what arrived first leaves first instead.
#[tokio::test]
async fn first_in_leaves_first_when_that_is_what_the_item_asked_for() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fifo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    for (code, expires, received) in [("keeps", 60, -30), ("soon", 3, -1)] {
        inventory::receive_lot(
            &mut tx,
            &ctx,
            item,
            location,
            inventory::NewLot {
                lot_code: code.into(),
                expires_at: Some(days(expires)),
                received_at: Some(days(received)),
                quantity: 1,
                supplier_reference: None,
            },
        )
        .await
        .expect("a delivery");
    }

    inventory::reserve(&mut tx, &ctx, item, location, 1, None, false, None)
        .await
        .expect("one of two");
    tx.commit().await.expect("to commit");

    assert_eq!(claimed(&shop, item).await, vec![("keeps".to_string(), 1)]);

    shop.close().await;
}

/// Out of date is out of stock, however much of it is on the shelf.
#[tokio::test]
async fn a_lot_that_is_out_of_date_is_never_promised() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "stale".into(),
            expires_at: Some(days(-1)),
            received_at: Some(days(-90)),
            quantity: 4,
            supplier_reference: None,
        },
    )
    .await
    .expect("a delivery nobody noticed going off");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let err = inventory::reserve(&mut tx, &shop.ctx(), item, location, 1, None, false, None)
        .await
        .expect_err("four on the shelf, none of them sellable");
    tx.rollback().await.expect("to roll back");

    assert_eq!(err.out_of_stock(), Some(item.as_uuid()));
    assert_eq!(stock(&shop, item, location).await, (4, 0, 4));

    let mut tx = shop.begin().await;
    let ageing =
        inventory::expiring_lots(&mut tx, &shop.ctx(), chrono::Utc::now(), Paging::first(10))
            .await
            .expect("the lots that are out of date");
    tx.commit().await.expect("to commit");
    assert_eq!(ageing.len(), 1, "nothing said the shelf had gone off");

    shop.close().await;
}

/// Two carts reach for the last unit of the only lot that is still good. The
/// level would let both of them through; the lot is what refuses one.
#[tokio::test]
async fn two_carts_race_for_the_last_unit_of_a_lot() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    for (code, expires) in [("good", 30), ("stale", -1)] {
        inventory::receive_lot(
            &mut tx,
            &ctx,
            item,
            location,
            inventory::NewLot {
                lot_code: code.into(),
                expires_at: Some(days(expires)),
                received_at: Some(days(-10)),
                quantity: 1,
                supplier_reference: None,
            },
        )
        .await
        .expect("a delivery");
    }
    tx.commit().await.expect("to commit");

    // The level says two are available. Only one of them may be sold.
    assert_eq!(stock(&shop, item, location).await, (2, 0, 2));

    async fn take(
        mut tx: Tx<'static>,
        ctx: Ctx<'_>,
        item: InventoryItemId,
        location: StockLocationId,
    ) -> tezgah::Result<()> {
        match inventory::reserve(&mut tx, &ctx, item, location, 1, None, false, None).await {
            Ok(_) => {
                tx.commit().await.map_err(tezgah::Error::from)?;
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.map_err(tezgah::Error::from)?;
                Err(err)
            }
        }
    }

    let one = shop.begin().await;
    let two = shop.begin().await;
    let (first, second) = tokio::join!(
        take(one, shop.ctx(), item, location),
        take(two, shop.ctx(), item, location),
    );

    let losers: Vec<_> = [&first, &second]
        .into_iter()
        .filter_map(|outcome| outcome.as_ref().err())
        .collect();

    assert_eq!(
        losers.len(),
        1,
        "exactly one of the two should have got the unit"
    );
    assert_eq!(losers[0].out_of_stock(), Some(item.as_uuid()));
    assert_eq!(claimed(&shop, item).await, vec![("good".to_string(), 1)]);

    shop.close().await;
}

/// Giving a hold back gives it back to the lot it was taken from, not to the
/// total.
#[tokio::test]
async fn releasing_a_hold_returns_it_to_the_lot_it_came_from() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "batch-1".into(),
            expires_at: Some(days(30)),
            received_at: None,
            quantity: 3,
            supplier_reference: None,
        },
    )
    .await
    .expect("a delivery");

    let held = inventory::reserve(&mut tx, &ctx, item, location, 2, None, false, None)
        .await
        .expect("two of three");
    tx.commit().await.expect("to commit");

    assert_eq!(claimed(&shop, item).await, vec![("batch-1".to_string(), 2)]);

    let mut tx = shop.begin().await;
    inventory::release(&mut tx, &shop.ctx(), held.id)
        .await
        .expect("to give it back");
    tx.commit().await.expect("to commit");

    assert!(claimed(&shop, item).await.is_empty());
    assert_eq!(stock(&shop, item, location).await, (3, 0, 3));

    let mut tx = shop.begin().await;
    let lots = inventory::lots_for_item(&mut tx, &shop.ctx(), item, None, Paging::first(10))
        .await
        .expect("the lots");
    tx.commit().await.expect("to commit");
    let lot = lots.items.first().expect("one lot");
    assert_eq!((lot.stocked_quantity, lot.reserved_quantity), (3, 0));

    shop.close().await;
}

/// A serial is a lot of one, and the same serial cannot arrive twice.
#[tokio::test]
async fn a_serial_names_one_unit_and_cannot_be_received_again() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fifo,
        inventory::TrackingMode::Serial,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let one = inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "SN-4401".into(),
            expires_at: None,
            received_at: None,
            quantity: 1,
            supplier_reference: None,
        },
    )
    .await
    .expect("one unit");
    assert!(one.is_serial);

    let two = inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "SN-4401".into(),
            expires_at: None,
            received_at: None,
            quantity: 1,
            supplier_reference: None,
        },
    )
    .await
    .expect_err("one unit cannot arrive twice");
    assert!(two.is_conflict());

    let many = inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "SN-4402".into(),
            expires_at: None,
            received_at: None,
            quantity: 5,
            supplier_reference: None,
        },
    )
    .await
    .expect_err("a serial is not a quantity");
    assert_eq!(many.code(), "invalid");

    tx.commit().await.expect("to commit");
    assert_eq!(stock(&shop, item, location).await, (1, 0, 1));

    shop.close().await;
}

/// The question nothing could answer: this lot went out — to whom?
#[tokio::test]
async fn a_recall_finds_the_orders_a_lot_went_out_in() {
    let shop = Shop::open().await;
    let (item, location, variant) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let recalled = inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "batch-4400".into(),
            expires_at: Some(days(10)),
            received_at: Some(days(-20)),
            quantity: 5,
            supplier_reference: Some("delivery note 7".into()),
        },
    )
    .await
    .expect("the batch that will be recalled");

    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        location,
        inventory::NewLot {
            lot_code: "batch-4500".into(),
            expires_at: Some(days(90)),
            received_at: Some(days(-2)),
            quantity: 5,
            supplier_reference: None,
        },
    )
    .await
    .expect("the batch that is fine");

    let order = OrderId::new();
    sqlx::query(r#"insert into "order" (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(order.as_uuid())
        .bind(shop.here.0)
        .execute(&mut *tx)
        .await
        .expect("an order");

    let line = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into order_line_item
             (id, scope, order_id, variant_id, title, unit_price, currency_code)
         values ($1, $2, $3, $4, 'A perishable thing', 100, 'TRY')",
    )
    .bind(line)
    .bind(shop.here.0)
    .bind(order.as_uuid())
    .bind(variant.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("a line item");

    let order_item = OrderItemId::new();
    sqlx::query(
        "insert into order_item
             (id, scope, order_id, order_line_item_id, currency_code, quantity, unit_price)
         values ($1, $2, $3, $4, 'TRY', 2, 100)",
    )
    .bind(order_item.as_uuid())
    .bind(shop.here.0)
    .bind(order.as_uuid())
    .bind(line)
    .execute(&mut *tx)
    .await
    .expect("an order item");

    fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        order,
        fulfilment::NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: false,
            created_by: Some("a test".into()),
            address: None,
            data: None,
            items: vec![fulfilment::NewFulfillmentItem {
                order_item_id: order_item,
                inventory_item_id: Some(item),
                title: "A perishable thing".into(),
                sku: None,
                barcode: None,
                quantity: 2,
            }],
        },
    )
    .await
    .expect("a parcel");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let went = inventory::orders_for_lot(&mut tx, &shop.ctx(), recalled.id, Paging::first(10))
        .await
        .expect("who got the recalled batch");
    tx.commit().await.expect("to commit");

    assert_eq!(
        went.len(),
        1,
        "the recalled batch went somewhere and nothing said where"
    );
    let shipment = went.items.first().expect("a shipment");
    assert_eq!(shipment.order_id, Some(order));
    assert_eq!(shipment.lot_code, "batch-4400");
    assert_eq!(shipment.quantity, 2);

    // FEFO shipped the earlier date, so the other batch is untouched.
    assert_eq!(stock(&shop, item, location).await, (8, 0, 8));

    shop.close().await;
}

/// Every lot row is somebody's, and it is not everybody's.
#[tokio::test]
async fn another_scope_sees_none_of_the_lots() {
    let shop = Shop::open().await;
    let (item, location, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;

    let mut tx = shop.begin().await;
    let made = inventory::receive_lot(
        &mut tx,
        &shop.ctx(),
        item,
        location,
        inventory::NewLot {
            lot_code: "batch-1".into(),
            expires_at: Some(days(30)),
            received_at: None,
            quantity: 4,
            supplier_reference: None,
        },
    )
    .await
    .expect("a delivery");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin_as(shop.elsewhere).await;
    let theirs = shop.theirs();

    assert!(
        inventory::lot(&mut tx, &theirs, made.id)
            .await
            .expect_err("not theirs")
            .is_not_found()
    );
    assert!(
        inventory::lots_for_item(&mut tx, &theirs, item, None, Paging::first(10))
            .await
            .expect("a page")
            .is_empty()
    );
    assert!(
        inventory::expiring_lots(&mut tx, &theirs, days(365), Paging::first(10))
            .await
            .expect("a page")
            .is_empty()
    );
    assert!(
        inventory::orders_for_lot(&mut tx, &theirs, made.id, Paging::first(10))
            .await
            .expect("a page")
            .is_empty()
    );
    assert!(
        inventory::reserve_from_lot(&mut tx, &theirs, made.id, 1, None, None)
            .await
            .expect_err("not theirs")
            .is_not_found()
    );
    tx.rollback().await.expect("to roll back");

    assert_eq!(stock(&shop, item, location).await, (4, 0, 4));

    shop.close().await;
}

// ---------------------------------------------------------------------------
// What a refusal is, and that it leaves the transaction usable.
// ---------------------------------------------------------------------------

/// A missing parent is a `not_found` rather than a conflict, a row something
/// still points at is a conflict — and neither of them takes the caller's
/// transaction down with it, which is what catching the constraint violation
/// after the fact used to do.
#[tokio::test]
async fn a_refused_write_says_which_of_the_three_it_was_and_the_transaction_lives() {
    let shop = Shop::open().await;
    let (item, location, variant) = seed(&shop, 3).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    // A parent that is not here.
    let missing = inventory::link_sales_channel(
        &mut tx,
        &ctx,
        StockLocationId::new(),
        tezgah::id::SalesChannelId::new(),
    )
    .await
    .expect_err("no such location");
    assert!(
        missing.is_not_found(),
        "a missing parent read as a conflict"
    );

    // Still usable: what used to happen here was 25P02 on the next statement.
    inventory::level(&mut tx, &ctx, item, location)
        .await
        .expect("the transaction to have survived a missing parent");

    let missing = inventory::attach_inventory_item(&mut tx, &ctx, VariantId::new(), item, 1)
        .await
        .expect_err("no such variant");
    assert!(missing.is_not_found());

    let missing = inventory::set_stock(&mut tx, &ctx, item, StockLocationId::new(), 1, 0)
        .await
        .expect_err("no such location");
    assert!(missing.is_not_found());

    // A name already taken is a genuine conflict, and stays one.
    let name = format!("depot {}", uuid::Uuid::now_v7());
    inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: name.clone(),
            address: None,
        },
    )
    .await
    .expect("a location");
    let taken = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name,
            address: None,
        },
    )
    .await
    .expect_err("that name is taken");
    assert!(taken.is_conflict());

    // A location stock is still counted at is a conflict too, and the condition
    // is in the delete rather than in a violation caught afterwards.
    let still_here = inventory::delete_stock_location(&mut tx, &ctx, location)
        .await
        .expect_err("stock is still counted there");
    assert!(still_here.is_conflict());

    // A location that is not here at all is a `not_found`, not that conflict.
    let gone = inventory::delete_stock_location(&mut tx, &ctx, StockLocationId::new())
        .await
        .expect_err("no such location");
    assert!(gone.is_not_found());

    // The whole run above left the transaction able to commit.
    let available = inventory::availability_for_variant(&mut tx, &ctx, variant, None)
        .await
        .expect("the transaction to still be usable");
    assert_eq!(available, 3);
    tx.commit().await.expect("to commit after six refusals");

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Transferring stock between locations (#147)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_transfer_moves_both_levels_in_one_transaction() {
    let shop = Shop::open().await;
    let (item, from, _) = seed(&shop, 10).await;
    let to = second_location(&shop).await;

    let mut tx = shop.begin().await;
    let transfer =
        inventory::transfer_stock(&mut tx, &shop.ctx(), item, from, to, 4, Some("restock"))
            .await
            .expect("four to move");
    tx.commit().await.expect("to commit");

    assert_eq!(transfer.quantity, 4);
    assert_eq!(transfer.from_location_id, from);
    assert_eq!(transfer.to_location_id, to);
    assert_eq!(transfer.status, "completed");

    // Neither double-counted nor lost: the sum across both locations is
    // exactly what it was before the move, and each side reads what the
    // transfer says it should.
    assert_eq!(stock(&shop, item, from).await, (6, 0, 6));
    assert_eq!(stock(&shop, item, to).await, (4, 0, 4));
    assert!(shop.host.audited("stock_transfer"));

    shop.close().await;
}

#[tokio::test]
async fn a_transfer_of_more_than_is_unpromised_changes_nothing() {
    let shop = Shop::open().await;
    let (item, from, _) = seed(&shop, 3).await;
    let to = second_location(&shop).await;

    let mut tx = shop.begin().await;
    let err = inventory::transfer_stock(&mut tx, &shop.ctx(), item, from, to, 5, None)
        .await
        .expect_err("more than the shelf has");
    tx.commit().await.expect("to commit");

    assert!(err.is_conflict());
    assert_eq!(stock(&shop, item, from).await, (3, 0, 3));

    let mut tx = shop.begin().await;
    let missing = inventory::level(&mut tx, &shop.ctx(), item, to).await;
    tx.commit().await.expect("to commit");
    assert!(
        missing.is_err(),
        "a refused transfer never touched the destination"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_transfer_leaves_reserved_stock_at_the_source() {
    let shop = Shop::open().await;
    let (item, from, _) = seed(&shop, 5).await;
    let to = second_location(&shop).await;

    let mut tx = shop.begin().await;
    inventory::reserve(&mut tx, &shop.ctx(), item, from, 4, None, false, None)
        .await
        .expect("four held for a sale");
    tx.commit().await.expect("to commit");

    // Only one unit is unpromised; asking for two must fail rather than ship
    // stock a sale is already counting on.
    let mut tx = shop.begin().await;
    let err = inventory::transfer_stock(&mut tx, &shop.ctx(), item, from, to, 2, None)
        .await
        .expect_err("more than what is unpromised");
    tx.commit().await.expect("to commit");
    assert!(err.is_conflict());
    assert_eq!(stock(&shop, item, from).await, (5, 4, 1));

    shop.close().await;
}

/// The claim this whole feature exists for: two transfers reaching for the
/// same last unit at once, and exactly one of them wins.
#[tokio::test]
async fn two_transfers_race_for_the_last_unit() {
    let shop = Shop::open().await;
    let (item, from, _) = seed(&shop, 1).await;
    let to = second_location(&shop).await;

    let one = shop.begin().await;
    let two = shop.begin().await;

    async fn move_one(
        mut tx: Tx<'static>,
        ctx: Ctx<'_>,
        item: InventoryItemId,
        from: StockLocationId,
        to: StockLocationId,
    ) -> tezgah::Result<()> {
        let outcome = inventory::transfer_stock(&mut tx, &ctx, item, from, to, 1, None).await;
        match outcome {
            Ok(_) => {
                tx.commit().await.map_err(tezgah::Error::from)?;
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.map_err(tezgah::Error::from)?;
                Err(err)
            }
        }
    }

    let (first, second) = tokio::join!(
        move_one(one, shop.ctx(), item, from, to),
        move_one(two, shop.ctx(), item, from, to),
    );

    let winners = [&first, &second].into_iter().filter(|o| o.is_ok()).count();
    let losers: Vec<_> = [&first, &second]
        .into_iter()
        .filter_map(|o| o.as_ref().err())
        .collect();

    assert_eq!(
        winners, 1,
        "exactly one transfer should have taken the unit"
    );
    assert_eq!(losers.len(), 1);
    assert!(losers[0].is_conflict());
    assert_eq!(stock(&shop, item, from).await, (0, 0, 0));
    assert_eq!(stock(&shop, item, to).await, (1, 0, 1));

    shop.close().await;
}

#[tokio::test]
async fn a_lot_tracked_transfer_moves_specific_lots_and_the_destination_knows_which() {
    let shop = Shop::open().await;
    let (item, from, _) = lot_seed(
        &shop,
        inventory::AllocationStrategy::Fefo,
        inventory::TrackingMode::Lot,
    )
    .await;
    let to = second_location(&shop).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        from,
        inventory::NewLot {
            lot_code: "early".into(),
            expires_at: Some(days(3)),
            received_at: None,
            quantity: 4,
            supplier_reference: None,
        },
    )
    .await
    .expect("the early batch");
    inventory::receive_lot(
        &mut tx,
        &ctx,
        item,
        from,
        inventory::NewLot {
            lot_code: "late".into(),
            expires_at: Some(days(10)),
            received_at: None,
            quantity: 4,
            supplier_reference: None,
        },
    )
    .await
    .expect("the late batch");
    tx.commit().await.expect("to commit the lots");

    // FEFO: the six units transferred come from the whole of "early" and two
    // of "late".
    let mut tx = shop.begin().await;
    inventory::transfer_stock(&mut tx, &shop.ctx(), item, from, to, 6, None)
        .await
        .expect("six units across two lots");
    tx.commit().await.expect("to commit");

    assert_eq!(stock(&shop, item, from).await, (2, 0, 2));
    assert_eq!(stock(&shop, item, to).await, (6, 0, 6));

    let mut tx = shop.begin().await;
    let arrived: Vec<(String, i32)> = sqlx::query_as(
        "select lot_code, stocked_quantity from inventory_lot
         where scope = $1 and inventory_item_id = $2 and location_id = $3
         order by lot_code",
    )
    .bind(shop.here.0)
    .bind(item.as_uuid())
    .bind(to.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the lots that arrived");
    tx.commit().await.expect("to commit");

    assert_eq!(
        arrived,
        vec![("early".to_string(), 4), ("late".to_string(), 2)],
        "the destination knows exactly which lots it received, and how much"
    );

    let mut tx = shop.begin().await;
    let recorded: Vec<(String, i32)> = sqlx::query_as(
        "select lot_code, quantity from stock_transfer_lot where scope = $1 order by lot_code",
    )
    .bind(shop.here.0)
    .fetch_all(&mut *tx)
    .await
    .expect("the transfer's own record of which lots moved");
    tx.commit().await.expect("to commit");
    assert_eq!(
        recorded,
        vec![("early".to_string(), 4), ("late".to_string(), 2)]
    );

    shop.close().await;
}

#[tokio::test]
async fn a_transfer_is_one_movement_not_two_adjustments() {
    let shop = Shop::open().await;
    let (item, from, _) = seed(&shop, 8).await;
    let to = second_location(&shop).await;

    let mut tx = shop.begin().await;
    let transfer = inventory::transfer_stock(&mut tx, &shop.ctx(), item, from, to, 3, None)
        .await
        .expect("three to move");
    tx.commit().await.expect("to commit");

    // One row, not two `inventory_level` adjustments: the audit says which
    // locations and how much moved, from a single write.
    let (transfer_audits, level_audits) = {
        let audits = shop.host.audits.lock();
        let transfer_audits = audits
            .iter()
            .filter(|(entity, id)| *entity == "stock_transfer" && *id == transfer.id.as_uuid())
            .count();
        let level_audits = audits
            .iter()
            .filter(|(entity, _)| *entity == "inventory_level")
            .count();
        (transfer_audits, level_audits)
    };
    assert_eq!(transfer_audits, 1);
    assert_eq!(
        level_audits, 0,
        "the level itself is never audited by a transfer; the transfer row is the record"
    );

    shop.close().await;
}
