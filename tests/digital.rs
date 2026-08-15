//! Selling a file: granted when the money arrives, taken back when it leaves,
//! and counted exactly once however many tabs are open.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::customer::{self, NewCustomer};
use tezgah::digital::{self, Access, NewContent};
use tezgah::id::{CustomerId, OrderId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::page::Paging;
use tezgah::ports::{Ctx, Tx};

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: Decimal) -> Money {
    Money::new(amount, lira())
}

async fn a_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, handle: &str) -> VariantId {
    let product = catalogue::create_product(
        tx,
        ctx,
        NewProduct {
            handle: handle.into(),
            title: format!("A {handle}"),
            ..NewProduct::default()
        },
    )
    .await
    .expect("a product");

    catalogue::create_variant(
        tx,
        ctx,
        product.id,
        NewVariant {
            title: "One size".into(),
            sku: Some(format!("{handle}-1")),
            ..NewVariant::default()
        },
    )
    .await
    .expect("a variant")
    .id
}

/// A variant carrying one file, with whatever limits the test wants on it.
async fn a_file_on(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
    max_downloads: Option<i32>,
) -> VariantId {
    let variant = a_variant(tx, ctx, handle).await;

    digital::put_content(
        tx,
        ctx,
        variant,
        NewContent {
            content_key: format!("books/{handle}.epub"),
            name: "The book".into(),
            max_downloads,
            ..NewContent::default()
        },
    )
    .await
    .expect("the file to go on the variant");

    variant
}

async fn an_order_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
    price: Decimal,
    customer_id: Option<CustomerId>,
) -> OrderId {
    order::create(
        tx,
        ctx,
        NewOrder {
            customer_id,
            lines: vec![NewOrderLine {
                variant_id: Some(variant),
                requires_shipping: false,
                ..NewOrderLine::of("The book", 1, money(price))
            }],
            ..NewOrder::of(lira())
        },
    )
    .await
    .expect("an order")
    .id
}

async fn money_arrived(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId, amount: Decimal) {
    order::record_transaction(
        tx,
        ctx,
        order_id,
        money(amount),
        "capture",
        uuid::Uuid::now_v7(),
    )
    .await
    .expect("the money to be recorded");
}

async fn entitlements_of(tx: &mut Tx<'_>, scope: uuid::Uuid, order_id: OrderId) -> i64 {
    sqlx::query_scalar("select count(*) from order_entitlement where scope = $1 and order_id = $2")
        .bind(scope)
        .bind(order_id.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .expect("the entitlements this order granted")
}

/// Checkout authorises; it does not take the money. Handing the file over
/// against a hold gives the goods away before the till rings.
#[tokio::test]
async fn an_authorised_order_grants_nothing() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "kettle-book", None).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(50), None).await;

    order::record_transaction(
        &mut tx,
        &ctx,
        order_id,
        money(dec!(50)),
        "payment",
        uuid::Uuid::now_v7(),
    )
    .await
    .expect("an authorisation");

    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("nothing to be granted");
    assert!(granted.is_empty(), "a hold is not a payment");
    assert_eq!(entitlements_of(&mut tx, shop.here.0, order_id).await, 0);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A provider redelivers its webhook, and the second delivery grants nothing.
#[tokio::test]
async fn taking_the_money_grants_exactly_once_however_often_it_is_told() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "atlas", Some(3)).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(50), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(50)).await;

    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");
    assert_eq!(granted.len(), 1);
    assert_eq!(
        granted[0].max_downloads,
        Some(3),
        "the limit is frozen on it"
    );
    assert_eq!(granted[0].content_key, "books/atlas.epub");

    let again = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the redelivery");
    assert!(again.is_empty(), "a doubled webhook grants once");
    assert_eq!(entitlements_of(&mut tx, shop.here.0, order_id).await, 1);
    assert!(shop.host.emitted("entitlement.granted"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Changing the file on the variant afterwards must not change what somebody
/// already bought.
#[tokio::test]
async fn what_was_bought_does_not_move_when_the_variant_does() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "manual", Some(2)).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(20), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(20)).await;
    digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");

    digital::put_content(
        &mut tx,
        &ctx,
        variant,
        NewContent {
            content_key: "books/manual.epub".into(),
            name: "The book, second edition".into(),
            max_downloads: Some(99),
            ..NewContent::default()
        },
    )
    .await
    .expect("the file to be edited");

    let held = digital::entitlements(&mut tx, &ctx, order_id)
        .await
        .expect("what the order holds");
    assert_eq!(held[0].max_downloads, Some(2), "frozen at the grant");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// The line every hand-rolled version forgets.
#[tokio::test]
async fn giving_the_money_back_takes_the_downloads_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "album", None).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(30), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(30)).await;
    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");

    let taken = digital::revoke(&mut tx, &ctx, order_id, Some("refunded"))
        .await
        .expect("the entitlement back");
    assert_eq!(taken.len(), 1);
    assert!(taken[0].revoked_at.is_some());
    assert!(shop.host.emitted("entitlement.revoked"));

    let refused = digital::issue_token(&mut tx, &ctx, granted[0].id, None)
        .await
        .expect_err("a withdrawn right mints no link");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_download_past_the_limit_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "single", Some(1)).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(10), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(10)).await;
    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");

    let link = digital::issue_token(&mut tx, &ctx, granted[0].id, None)
        .await
        .expect("a link");
    let taken = digital::redeem(&mut tx, &ctx, &link.token, Access::default())
        .await
        .expect("the one download");
    assert_eq!(taken.content_key, "books/single.epub");
    assert_eq!(taken.remaining, Some(0));
    assert!(shop.host.emitted("entitlement.downloads_exhausted"));

    let refused = digital::redeem(&mut tx, &ctx, &link.token, Access::default())
        .await
        .expect_err("the second download");
    assert!(refused.is_conflict());

    // The refusal is written down: a chargeback is answered from this table.
    let attempts: i64 = sqlx::query_scalar(
        "select count(*) from entitlement_access
         where scope = $1 and order_entitlement_id = $2 and outcome = 'refused'",
    )
    .bind(shop.here.0)
    .bind(granted[0].id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the access log");
    assert_eq!(attempts, 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_download_past_the_expiry_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "lecture", None).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(10), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(10)).await;
    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");

    let link = digital::issue_token(&mut tx, &ctx, granted[0].id, None)
        .await
        .expect("a link");

    sqlx::query("update order_entitlement set expires_at = now() - interval '1 day' where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(granted[0].id.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the right to have run out");

    let refused = digital::redeem(&mut tx, &ctx, &link.token, Access::default())
        .await
        .expect_err("a right that has run out");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Two tabs, at the same moment, on the last download there is. Two
/// connections rather than two calls in a row: a race simulated in sequence
/// proves nothing.
#[tokio::test]
async fn two_connections_take_the_last_download_and_exactly_one_wins() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let variant = a_file_on(&mut tx, &ctx, "ticket", Some(1)).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(10), None).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(10)).await;
    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");
    let link = digital::issue_token(&mut tx, &ctx, granted[0].id, None)
        .await
        .expect("a link");
    let token = link.token.clone();
    tx.commit().await.expect("to commit");

    let one = async {
        let ctx = shop.ctx();
        let mut tx = shop.begin().await;
        let outcome = digital::redeem(&mut tx, &ctx, &token, Access::default()).await;
        tx.commit().await.expect("to commit");
        outcome
    };
    let two = async {
        let ctx = shop.ctx();
        let mut tx = shop.begin().await;
        let outcome = digital::redeem(&mut tx, &ctx, &token, Access::default()).await;
        tx.commit().await.expect("to commit");
        outcome
    };

    let (first, second) = tokio::join!(one, two);
    let won = [&first, &second].iter().filter(|out| out.is_ok()).count();
    assert_eq!(
        won, 1,
        "both tabs got the last download: {first:?} {second:?}"
    );

    let mut tx = shop.begin().await;
    let after = digital::entitlements(&mut tx, &ctx, order_id)
        .await
        .expect("what the order holds");
    assert_eq!(after[0].download_count, 1, "counted once, not twice");
    tx.rollback().await.expect("to roll back");

    shop.close().await;
}

/// A basket holding a book and its audiobook: the file half is delivered the
/// instant the money clears, and the parcel half stays the operator's problem.
#[tokio::test]
async fn a_mixed_order_grants_its_digital_half_and_leaves_the_parcel_alone() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let file = a_file_on(&mut tx, &ctx, "audiobook", None).await;
    let parcel = a_variant(&mut tx, &ctx, "hardback").await;

    let order_id = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            lines: vec![
                NewOrderLine {
                    variant_id: Some(file),
                    requires_shipping: false,
                    ..NewOrderLine::of("The audiobook", 1, money(dec!(20)))
                },
                NewOrderLine {
                    variant_id: Some(parcel),
                    ..NewOrderLine::of("The hardback", 1, money(dec!(30)))
                },
            ],
            ..NewOrder::of(lira())
        },
    )
    .await
    .expect("an order")
    .id;

    money_arrived(&mut tx, &ctx, order_id, dec!(50)).await;

    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the digital half");
    assert_eq!(granted.len(), 1, "only the line carrying a file is granted");

    let line: uuid::Uuid = sqlx::query_scalar(
        "select id from order_line_item where scope = $1 and order_id = $2 and variant_id = $3",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .bind(file.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the digital line");
    assert_eq!(granted[0].order_line_item_id.as_uuid(), line);

    // Nothing was fulfilled off a shelf, and the parcel's counters did not move.
    let shipped: i64 = sqlx::query_scalar(
        r#"select coalesce(sum(i.fulfilled_quantity), 0) from order_item i
           join "order" o on o.scope = i.scope and o.id = i.order_id and o.version = i.version
           where i.scope = $1 and i.order_id = $2"#,
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("what has been fulfilled");
    assert_eq!(shipped, 0, "granting a file does not despatch a parcel");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn somebody_elses_library_is_neither_readable_nor_spendable() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let buyer = customer::create(&mut tx, &ctx, NewCustomer::guest())
        .await
        .expect("the buyer")
        .id;
    let stranger = customer::create(&mut tx, &ctx, NewCustomer::guest())
        .await
        .expect("somebody else")
        .id;

    let variant = a_file_on(&mut tx, &ctx, "course", None).await;
    let order_id = an_order_for(&mut tx, &ctx, variant, dec!(40), Some(buyer)).await;
    money_arrived(&mut tx, &ctx, order_id, dec!(40)).await;
    let granted = digital::grant(&mut tx, &ctx, order_id)
        .await
        .expect("the entitlement");
    assert_eq!(granted[0].customer_id, Some(buyer));

    let mine = digital::for_customer(&mut tx, &ctx, stranger, Paging::first(10))
        .await
        .expect("the stranger's library");
    assert!(mine.is_empty(), "somebody else's book is not in it");

    let refused = digital::issue_token(&mut tx, &ctx, granted[0].id, Some(stranger))
        .await
        .expect_err("a link on somebody else's right");
    assert!(refused.is_not_found());

    let link = digital::issue_token(&mut tx, &ctx, granted[0].id, Some(buyer))
        .await
        .expect("the buyer's own link");
    let refused = digital::redeem(
        &mut tx,
        &ctx,
        &link.token,
        Access {
            as_customer: Some(stranger),
            ..Access::default()
        },
    )
    .await
    .expect_err("a stolen link is not the stranger's to spend");
    assert!(refused.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
