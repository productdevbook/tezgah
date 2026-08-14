//! A cart that changed is a cart worked out again, against a real Postgres.
//!
//! Both halves of this were wrong in a way only running it showed: a discount
//! stayed at what the cart used to hold, and the tax lines every total sums
//! were never written at all, so every cart was silently taxed at nothing.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::api::store::{self, AddLineItem, AddressInput, CreateCart, UpdateCart, UpdateLineItem};
use tezgah::cart;
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::id::{CartId, PromotionId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::ports::{Ctx, Tx};
use tezgah::pricing::{self, NewPrice};
use tezgah::promotion::{
    self, Allocation, MethodKind, NewApplicationMethod, NewPromotion, PromotionKind, Status,
    TargetKind,
};
use tezgah::tax::{self, NewTaxRate, NewTaxRegion};
use uuid::Uuid;

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

async fn seed_currency(tx: &mut Tx<'_>, scope: Uuid) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, '₺', '₺', 'Turkish lira')",
    )
    .bind(Uuid::now_v7())
    .bind(scope)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

/// A published variant priced at a hundred lira.
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

    catalogue::publish_product(tx, ctx, product.id)
        .await
        .expect("to publish it");

    let variant = catalogue::create_variant(
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
    .expect("a variant");

    let set = pricing::create_price_set(tx, ctx)
        .await
        .expect("a price set");
    pricing::link_variant(tx, ctx, variant.id, set.id)
        .await
        .expect("to link it");
    pricing::add_price(
        tx,
        ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(dec!(100), lira()),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await
    .expect("a price");

    variant.id
}

/// A country charging eighteen percent on everything in it.
async fn eighteen_percent(tx: &mut Tx<'_>, ctx: &Ctx<'_>) {
    let region = tax::create_tax_region(
        tx,
        ctx,
        NewTaxRegion {
            country_code: "TR".into(),
            province_code: None,
            parent_id: None,
            provider: None,
        },
    )
    .await
    .expect("a tax region");

    tax::create_tax_rate(
        tx,
        ctx,
        NewTaxRate {
            tax_region_id: region.id,
            rate: dec!(18),
            code: Some("vat".into()),
            name: "VAT".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("a tax rate");
}

/// Ten percent off everything, applied without a code.
async fn ten_percent_off(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> PromotionId {
    let made = promotion::create_promotion(
        tx,
        ctx,
        NewPromotion {
            code: "TENPERCENT".into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: true,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: None,
        },
    )
    .await
    .expect("a promotion");

    promotion::set_application_method(
        tx,
        ctx,
        NewApplicationMethod {
            promotion_id: made.id,
            kind: MethodKind::Percentage,
            target_type: TargetKind::Order,
            allocation: Some(Allocation::Each),
            value: dec!(10),
            currency_code: Some(lira()),
            max_quantity: None,
            apply_to_quantity: None,
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect("an application method");

    made.id
}

async fn a_cart(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> CartId {
    store::create_cart(
        tx,
        ctx,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await
    .expect("a cart")
    .id
}

async fn delivering_to_turkey(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart: CartId) {
    store::update_cart(
        tx,
        ctx,
        cart,
        UpdateCart {
            email: None,
            shipping_address: Some(AddressInput {
                country_code: Some("TR".into()),
                ..AddressInput::default()
            }),
            billing_address: None,
        },
    )
    .await
    .expect("an address");
}

async fn adjustments(tx: &mut Tx<'_>, scope: Uuid, cart: CartId) -> Decimal {
    sqlx::query_scalar(
        "select coalesce(sum(a.amount), 0)
         from cart_line_item_adjustment a
         join cart_line_item l on l.scope = a.scope and l.id = a.cart_line_item_id
         where a.scope = $1 and l.cart_id = $2",
    )
    .bind(scope)
    .bind(cart.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the adjustments")
}

async fn tax_lines(tx: &mut Tx<'_>, scope: Uuid, cart: CartId) -> i64 {
    sqlx::query_scalar(
        "select count(*)
         from cart_line_item_tax_line t
         join cart_line_item l on l.scope = t.scope and l.id = t.cart_line_item_id
         where t.scope = $1 and l.cart_id = $2",
    )
    .bind(scope)
    .bind(cart.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the tax lines")
}

#[tokio::test]
async fn a_line_changed_is_discounted_again() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    ten_percent_off(&mut tx, &ctx).await;
    let cart = a_cart(&mut tx, &ctx).await;

    let line = store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");

    assert_eq!(
        adjustments(&mut tx, shop.here.0, cart).await,
        dec!(10.00),
        "adding a line priced the cart again"
    );

    store::update_line_item(
        &mut tx,
        &ctx,
        cart,
        line.id,
        UpdateLineItem { quantity: 10 },
    )
    .await
    .expect("a bigger line");

    assert_eq!(
        adjustments(&mut tx, shop.here.0, cart).await,
        dec!(100.00),
        "ten of them at ten percent off is a hundred, not one unit's worth"
    );

    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.discount.amount, dec!(100.00));

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_line_removed_takes_its_discount_with_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let kettle = a_variant(&mut tx, &ctx, "kettle").await;
    let kilim = a_variant(&mut tx, &ctx, "kilim").await;
    ten_percent_off(&mut tx, &ctx).await;
    let cart = a_cart(&mut tx, &ctx).await;

    let first = store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: kettle,
            quantity: 1,
        },
    )
    .await
    .expect("a line");

    store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: kilim,
            quantity: 1,
        },
    )
    .await
    .expect("a second line");

    assert_eq!(adjustments(&mut tx, shop.here.0, cart).await, dec!(20.00));

    store::remove_line_item(&mut tx, &ctx, cart, first.id)
        .await
        .expect("to take one back out");

    assert_eq!(
        adjustments(&mut tx, shop.here.0, cart).await,
        dec!(10.00),
        "the discount shrank with the cart"
    );

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_promotion_that_no_longer_holds_leaves_nothing_behind() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    let promotion = ten_percent_off(&mut tx, &ctx).await;
    let cart = a_cart(&mut tx, &ctx).await;

    let line = store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");
    assert_eq!(adjustments(&mut tx, shop.here.0, cart).await, dec!(10.00));

    promotion::set_status(&mut tx, &ctx, promotion, Status::Inactive)
        .await
        .expect("to withdraw it");

    store::update_line_item(&mut tx, &ctx, cart, line.id, UpdateLineItem { quantity: 2 })
        .await
        .expect("a bigger line");

    assert_eq!(
        adjustments(&mut tx, shop.here.0, cart).await,
        dec!(0),
        "a promotion that no longer applies is off the cart"
    );

    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.discount.amount, dec!(0.00));

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn tax_lines_are_written_and_the_totals_add_them_up() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    eighteen_percent(&mut tx, &ctx).await;
    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    let cart = a_cart(&mut tx, &ctx).await;
    delivering_to_turkey(&mut tx, &ctx, cart).await;

    store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");

    assert_eq!(
        tax_lines(&mut tx, shop.here.0, cart).await,
        1,
        "the answer tax worked out was written down"
    );

    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.subtotal.amount, dec!(100.00));
    assert_eq!(totals.tax.amount, dec!(18.00));
    assert_eq!(totals.total.amount, dec!(118.00));

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn an_inclusive_price_pays_its_tax_once() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    eighteen_percent(&mut tx, &ctx).await;
    pricing::set_price_preference(&mut tx, &ctx, "currency_code", Some("TRY".into()), true)
        .await
        .expect("prices that carry their tax");

    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    let cart = a_cart(&mut tx, &ctx).await;
    delivering_to_turkey(&mut tx, &ctx, cart).await;

    store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");

    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.subtotal.amount, dec!(84.75));
    assert_eq!(totals.tax.amount, dec!(15.25));
    assert_eq!(
        totals.total.amount,
        dec!(100.00),
        "a hundred lira on the shelf is a hundred lira at the till"
    );

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_cart_with_nowhere_to_go_carries_no_tax() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    eighteen_percent(&mut tx, &ctx).await;
    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    let cart = a_cart(&mut tx, &ctx).await;

    store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");

    assert_eq!(
        tax_lines(&mut tx, shop.here.0, cart).await,
        0,
        "no address is no jurisdiction, and no tax line rather than a refusal"
    );
    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.tax.amount, dec!(0.00));
    assert_eq!(totals.total.amount, dec!(100.00));

    delivering_to_turkey(&mut tx, &ctx, cart).await;

    assert_eq!(
        tax_lines(&mut tx, shop.here.0, cart).await,
        1,
        "and the address arriving is what taxes it"
    );
    let totals = cart::totals(&mut tx, &ctx, cart).await.expect("the totals");
    assert_eq!(totals.tax.amount, dec!(18.00));

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn another_scope_reprices_nothing_of_ours() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    eighteen_percent(&mut tx, &ctx).await;
    let variant = a_variant(&mut tx, &ctx, "kettle").await;
    let cart = a_cart(&mut tx, &ctx).await;
    delivering_to_turkey(&mut tx, &ctx, cart).await;

    store::add_line_item(
        &mut tx,
        &ctx,
        cart,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
        },
    )
    .await
    .expect("a line");
    tx.commit().await.expect("to commit");

    let mut elsewhere = shop.begin_as(shop.elsewhere).await;
    let theirs = shop.theirs();

    assert!(
        store::reprice(&mut elsewhere, &theirs, cart)
            .await
            .expect_err("that cart is another shop's")
            .is_not_found()
    );
    assert_eq!(
        tax_lines(&mut elsewhere, shop.elsewhere.0, cart).await,
        0,
        "nor are its tax lines visible from here"
    );

    drop(elsewhere);

    let mut mine = shop.begin().await;
    assert_eq!(
        tax_lines(&mut mine, shop.here.0, cart).await,
        1,
        "and ours are still ours"
    );
    drop(mine);

    shop.close().await;
}
