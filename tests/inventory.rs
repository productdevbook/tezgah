//! Stock, against a real Postgres.
//!
//! The one that matters is `two_carts_race_for_the_last_unit`: two connections
//! reaching for the same unit at the same moment, on two transactions that are
//! genuinely open together. Doing one after the other would pass whatever the
//! code did.

mod common;

use common::Shop;
use tezgah::id::{InventoryItemId, StockLocationId, VariantId};
use tezgah::inventory;
use tezgah::page::Paging;
use tezgah::ports::{Ctx, Scope, Tx};

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
