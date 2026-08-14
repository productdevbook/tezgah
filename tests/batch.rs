//! Many rows at once, against a real Postgres.
//!
//! The question these ask is not whether a thousand good rows go in. It is what
//! happens to the nine hundred and ninety-seven when three of them are wrong.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use tezgah::batch;
use tezgah::id::PriceId;
use tezgah::money::{Currency, Money};
use tezgah::page::{Cursor, Paging};

fn row(at: usize) -> batch::ProductRow {
    batch::ProductRow {
        handle: format!("thing-{at}"),
        title: format!("Thing {at}"),
        ..batch::ProductRow::default()
    }
}

fn priced(handle: &str, sku: &str, amount: i64, currency: &str) -> batch::ProductRow {
    batch::ProductRow {
        handle: handle.into(),
        title: format!("Product {handle}"),
        variant_title: Some(sku.into()),
        sku: Some(sku.into()),
        price_amount: Some(Decimal::from(amount)),
        price_currency: Some(currency.into()),
        ..batch::ProductRow::default()
    }
}

#[tokio::test]
async fn three_bad_rows_in_a_thousand_are_named_and_the_rest_go_in() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let mut rows: Vec<batch::ProductRow> = (0..1_000).map(row).collect();
    rows[10].title = String::new();
    rows[500].handle = String::new();
    rows[999].price_amount = Some(Decimal::from(5));

    let result = batch::import_products(
        &mut tx,
        &ctx,
        batch::ImportProducts {
            rows,
            delete: Vec::new(),
        },
    )
    .await
    .expect("an import that reports rather than fails");

    assert_eq!(result.created, 997);
    assert_eq!(result.updated, 0);

    let refused: Vec<usize> = result.rejected.iter().map(|one| one.row).collect();
    assert_eq!(refused, vec![10, 500, 999]);
    assert!(
        result.rejected.iter().all(|one| !one.reason.is_empty()),
        "a rejection has to say why"
    );

    // The good rows are really there: the one after a bad one included.
    let page = batch::export_products(&mut tx, &ctx, None, Paging::first(1))
        .await
        .expect("to export");
    assert_eq!(page.len(), 0, "no variants were asked for by these rows");

    let count: i64 = sqlx::query_scalar("select count(*) from product where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("to count products");
    assert_eq!(count, 997);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_batch_over_the_ceiling_is_refused_whole() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let rows: Vec<batch::ProductRow> = (0..batch::MAX_BATCH + 1).map(row).collect();
    let err = batch::import_products(
        &mut tx,
        &ctx,
        batch::ImportProducts {
            rows,
            delete: Vec::new(),
        },
    )
    .await
    .expect_err("more than the ceiling to be refused");
    assert_eq!(err.code(), "invalid");

    let count: i64 = sqlx::query_scalar("select count(*) from product where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("to count products");
    assert_eq!(count, 0, "a refused batch writes nothing");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn the_export_pages_and_covers_every_variant() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let mut rows = Vec::new();
    for product in 0..3 {
        for variant in 0..2 {
            rows.push(priced(
                &format!("shirt-{product}"),
                &format!("sku-{product}-{variant}"),
                10 + product,
                "EUR",
            ));
        }
    }

    let result = batch::import_products(
        &mut tx,
        &ctx,
        batch::ImportProducts {
            rows,
            delete: Vec::new(),
        },
    )
    .await
    .expect("an import");
    assert_eq!(result.rejected.len(), 0);
    assert_eq!(result.created, 3);
    assert_eq!(result.updated, 3);

    let mut seen = Vec::new();
    let mut after: Option<Cursor> = None;
    loop {
        let paging = match after {
            Some(cursor) => Paging::after(cursor, 2),
            None => Paging::first(2),
        };
        let page = batch::export_products(&mut tx, &ctx, None, paging)
            .await
            .expect("a page of the export");
        assert!(page.len() <= 2, "a page bigger than was asked for");

        for line in &page.items {
            assert_eq!(line.price_currency.as_deref(), Some("EUR"));
            assert!(
                line.price_amount.is_some(),
                "a variant exported with no price"
            );
            seen.push(line.sku.clone().unwrap_or_default());
        }

        match &page.next {
            Some(next) => after = Some(Cursor::decode(next).expect("its own cursor")),
            None => break,
        }
    }

    seen.sort();
    assert_eq!(seen.len(), 6, "every variant has to be on some page");
    seen.dedup();
    assert_eq!(seen.len(), 6, "a variant came back on two pages");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_price_batch_carrying_two_currencies_is_refused() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    batch::import_products(
        &mut tx,
        &ctx,
        batch::ImportProducts {
            rows: vec![
                priced("euro", "sku-eur", 10, "EUR"),
                priced("dollar", "sku-usd", 20, "USD"),
            ],
            delete: Vec::new(),
        },
    )
    .await
    .expect("an import");

    let held: Vec<(PriceId, String)> = sqlx::query_as(
        "select id, currency_code from price where scope = $1 order by currency_code",
    )
    .bind(shop.here.0)
    .fetch_all(&mut *tx)
    .await
    .expect("the prices the import wrote");
    assert_eq!(held.len(), 2);

    let mixed: Vec<batch::PriceChange> = held
        .iter()
        .map(|(id, code)| batch::PriceChange {
            price_id: *id,
            amount: Money::new(
                Decimal::from(99),
                Currency::parse(code).expect("a currency"),
            ),
        })
        .collect();

    let err = batch::update_prices(&mut tx, &ctx, mixed)
        .await
        .expect_err("two currencies in one batch to be refused");
    assert_eq!(err.code(), "invalid");

    let unmoved: Decimal = sqlx::query_scalar("select amount from price where id = $1")
        .bind(held[0].0.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .expect("the price");
    assert_eq!(unmoved, Decimal::from(10), "a refused batch moved a price");

    // One currency at a time is fine, and a price that is not there is a row
    // rejection rather than the end of the batch.
    let result = batch::update_prices(
        &mut tx,
        &ctx,
        vec![
            batch::PriceChange {
                price_id: held[0].0,
                amount: Money::new(Decimal::from(11), Currency::parse("EUR").expect("EUR")),
            },
            batch::PriceChange {
                price_id: PriceId::new(),
                amount: Money::new(Decimal::from(12), Currency::parse("EUR").expect("EUR")),
            },
        ],
    )
    .await
    .expect("a batch of one currency");

    assert_eq!(result.applied, 1);
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].row, 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_stock_take_applies_the_rows_it_can_and_names_the_rest() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let location = tezgah::inventory::create_stock_location(
        &mut tx,
        &ctx,
        tezgah::inventory::NewStockLocation {
            name: "a warehouse".into(),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = tezgah::inventory::create_inventory_item(
        &mut tx,
        &ctx,
        tezgah::inventory::NewInventoryItem {
            sku: Some("sku-stock".into()),
            title: Some("a thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    let result = batch::set_stock_levels(
        &mut tx,
        &ctx,
        vec![
            batch::StockLevelRow {
                inventory_item_id: item.id,
                location_id: location.id,
                stocked_quantity: 7,
                incoming_quantity: None,
            },
            batch::StockLevelRow {
                inventory_item_id: item.id,
                location_id: location.id,
                stocked_quantity: -1,
                incoming_quantity: None,
            },
        ],
    )
    .await
    .expect("a stock take");

    assert_eq!(result.applied, 1);
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].row, 1);

    let level = tezgah::inventory::level(&mut tx, &ctx, item.id, location.id)
        .await
        .expect("the level");
    assert_eq!(level.stocked_quantity, 7);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn another_scope_sees_none_of_it() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let ctx = shop.ctx();
    batch::import_products(
        &mut mine,
        &ctx,
        batch::ImportProducts {
            rows: vec![priced("secret", "sku-secret", 10, "EUR")],
            delete: Vec::new(),
        },
    )
    .await
    .expect("an import");

    let held: PriceId = sqlx::query_scalar("select id from price where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *mine)
        .await
        .expect("the price");
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let other = shop.theirs();

    let page = batch::export_products(&mut theirs, &other, None, Paging::first(50))
        .await
        .expect("an export of their own shop");
    assert_eq!(page.len(), 0, "somebody else's variants were exported");

    let result = batch::update_prices(
        &mut theirs,
        &other,
        vec![batch::PriceChange {
            price_id: held,
            amount: Money::new(Decimal::from(1), Currency::parse("EUR").expect("EUR")),
        }],
    )
    .await
    .expect("a batch that finds nothing of its own");
    assert_eq!(result.applied, 0);
    assert_eq!(result.rejected.len(), 1);

    theirs.rollback().await.expect("to roll back");

    let mut back = shop.begin().await;
    let still: Decimal = sqlx::query_scalar("select amount from price where id = $1")
        .bind(held.as_uuid())
        .fetch_one(&mut *back)
        .await
        .expect("the price");
    assert_eq!(still, Decimal::from(10));
    back.rollback().await.expect("to roll back");

    shop.close().await;
}
