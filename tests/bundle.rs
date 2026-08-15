//! A bundle's price, against a real Postgres.
//!
//! Stock is already composable through `variant_inventory_item` (#87's
//! survey found that solved). What these prove is the rest of it: a bundle's
//! price is allocated across its components with `Money::allocate`, so the
//! parts add back up to the whole; a component keeps its own tax rate rather
//! than the bundle's blended one; and a cart carries the allocation onto its
//! lines rather than pricing the bundle as one opaque thing.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::cart::{self, AddBundle, AddBundleComponent, AddLine};
use tezgah::catalogue;
use tezgah::id::VariantId;
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine, OrderAddress, ReturnLine};
use tezgah::ports::{Ctx, Scope, Tx};
use tezgah::pricing::{self, BundlePriceMode, NewBundleComponent, NewPrice, PriceContext};
use tezgah::tax::{self, NewTaxRate, NewTaxRateRule, NewTaxRegion, TaxReference, TaxableAddress};

async fn line_count(tx: &mut Tx<'_>, cart_id: tezgah::id::CartId) -> i64 {
    sqlx::query_scalar("select count(*) from cart_line_item where cart_id = $1")
        .bind(cart_id.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .expect("to count the cart's lines")
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
    .bind(uuid::Uuid::now_v7())
    .bind(scope.0)
    .execute(&mut **tx)
    .await
    .expect("a currency row");
}

/// A variant with its own price set and a price on it, so it can stand as a
/// bundle component or as the bundle itself.
async fn a_priced_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
    amount: Decimal,
) -> VariantId {
    let product = catalogue::create_product(
        tx,
        ctx,
        catalogue::NewProduct {
            handle: handle.into(),
            title: format!("A {handle}"),
            ..catalogue::NewProduct::default()
        },
    )
    .await
    .expect("a product");

    let variant = catalogue::create_variant(
        tx,
        ctx,
        product.id,
        catalogue::NewVariant {
            title: "One size".into(),
            sku: Some(format!("{handle}-1")),
            ..catalogue::NewVariant::default()
        },
    )
    .await
    .expect("a variant");

    let set = pricing::create_price_set(tx, ctx)
        .await
        .expect("a price set");
    pricing::link_variant(tx, ctx, variant.id, set.id)
        .await
        .expect("to link the price set");
    pricing::add_price(
        tx,
        ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: money(amount),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await
    .expect("a price");

    variant.id
}

#[tokio::test]
async fn a_components_priced_bundle_allocates_and_sums_back_up() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let candle = a_priced_variant(&mut tx, &ctx, "candle", dec!(30)).await;
    let soap = a_priced_variant(&mut tx, &ctx, "soap", dec!(20)).await;
    let bundle = a_priced_variant(&mut tx, &ctx, "gift-set", dec!(0)).await;

    pricing::set_bundle_price_mode(
        &mut tx,
        &ctx,
        bundle,
        Some(BundlePriceMode::Components),
        Some(dec!(10)),
    )
    .await
    .expect("to mark it a bundle");

    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: candle,
            quantity: 1,
            sort_order: 0,
        },
    )
    .await
    .expect("a component");
    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: soap,
            quantity: 2,
            sort_order: 1,
        },
    )
    .await
    .expect("a second component");

    let price = pricing::resolve_bundle(&mut tx, &ctx, bundle, &PriceContext::new(lira(), 1))
        .await
        .expect("resolution to run")
        .expect("a bundle price");

    // 30 + 2*20 = 70, minus 10% is 63.
    assert_eq!(price.total.amount, dec!(63.00));
    assert_eq!(price.components.len(), 2);

    let summed: Decimal = price
        .components
        .iter()
        .map(|c| c.allocated_total.amount)
        .sum();
    assert_eq!(summed, price.total.amount, "the parts did not add back up");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_fixed_price_bundle_is_still_allocated_across_its_components() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let candle = a_priced_variant(&mut tx, &ctx, "candle", dec!(30)).await;
    let soap = a_priced_variant(&mut tx, &ctx, "soap", dec!(20)).await;
    let bundle = a_priced_variant(&mut tx, &ctx, "gift-set", dec!(45)).await;

    pricing::set_bundle_price_mode(&mut tx, &ctx, bundle, Some(BundlePriceMode::Fixed), None)
        .await
        .expect("to mark it a bundle");

    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: candle,
            quantity: 1,
            sort_order: 0,
        },
    )
    .await
    .expect("a component");
    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: soap,
            quantity: 1,
            sort_order: 1,
        },
    )
    .await
    .expect("a second component");

    let price = pricing::resolve_bundle(&mut tx, &ctx, bundle, &PriceContext::new(lira(), 1))
        .await
        .expect("resolution to run")
        .expect("a bundle price");

    assert_eq!(price.total.amount, dec!(45));
    let summed: Decimal = price
        .components
        .iter()
        .map(|c| c.allocated_total.amount)
        .sum();
    assert_eq!(
        summed,
        dec!(45),
        "a fixed bundle's parts did not add back up"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn bundle_components_at_different_tax_rates_are_taxed_at_their_own_rate() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let country = tax::create_tax_region(
        &mut tx,
        &ctx,
        NewTaxRegion {
            country_code: "TR".into(),
            province_code: None,
            parent_id: None,
            provider: None,
        },
    )
    .await
    .expect("a country");

    let _standard = tax::create_tax_rate(
        &mut tx,
        &ctx,
        NewTaxRate {
            tax_region_id: country.id,
            rate: dec!(20),
            code: Some("standard".into()),
            name: "Standard".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("the default rate");

    let reduced = tax::create_tax_rate(
        &mut tx,
        &ctx,
        NewTaxRate {
            tax_region_id: country.id,
            rate: dec!(1),
            code: Some("reduced".into()),
            name: "Reduced".into(),
            is_default: false,
            is_combinable: false,
        },
    )
    .await
    .expect("a reduced rate");

    // A book taxed at 1% and a candle taxed at the 20% default, sold as one
    // bundle. The bundle itself is never a tax target: each component is.
    let book = a_priced_variant(&mut tx, &ctx, "book", dec!(50)).await;
    let candle = a_priced_variant(&mut tx, &ctx, "candle", dec!(50)).await;
    let bundle = a_priced_variant(&mut tx, &ctx, "reading-set", dec!(0)).await;

    tax::create_tax_rate_rule(
        &mut tx,
        &ctx,
        NewTaxRateRule {
            tax_rate_id: reduced.id,
            reference: TaxReference::Variant,
            reference_id: book.as_uuid(),
        },
    )
    .await
    .expect("a variant rule");

    pricing::set_bundle_price_mode(
        &mut tx,
        &ctx,
        bundle,
        Some(BundlePriceMode::Components),
        None,
    )
    .await
    .expect("to mark it a bundle");
    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: book,
            quantity: 1,
            sort_order: 0,
        },
    )
    .await
    .expect("a component");
    pricing::add_bundle_component(
        &mut tx,
        &ctx,
        bundle,
        NewBundleComponent {
            component_variant_id: candle,
            quantity: 1,
            sort_order: 1,
        },
    )
    .await
    .expect("a second component");

    let price = pricing::resolve_bundle(&mut tx, &ctx, bundle, &PriceContext::new(lira(), 1))
        .await
        .expect("resolution to run")
        .expect("a bundle price");
    assert_eq!(price.total.amount, dec!(100));

    let lines: Vec<tax::TaxableLine> = price
        .components
        .iter()
        .map(|c| tax::TaxableLine {
            id: uuid::Uuid::now_v7(),
            amount: c.allocated_total,
            targets: vec![tax::TaxTarget {
                reference: TaxReference::Variant,
                id: c.component_variant_id.as_uuid(),
            }],
            tax_code: None,
            address: None,
        })
        .collect();

    let address = TaxableAddress {
        country_code: "TR".into(),
        province_code: None,
        postal_code: None,
    };

    let out = tax::calculate(&mut tx, &ctx, &lines, &address, None, false)
        .await
        .expect("a calculation");

    assert_eq!(out.len(), 2);
    let rates: std::collections::BTreeSet<Decimal> = out.iter().map(|l| l.rate).collect();
    assert_eq!(
        rates,
        std::collections::BTreeSet::from([dec!(1), dec!(20)]),
        "each component kept its own rate rather than a blended one"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn add_bundle_prices_a_zero_parent_and_children_that_sum_to_the_bundle_total() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let candle = a_priced_variant(&mut tx, &ctx, "candle", dec!(30)).await;
    let soap = a_priced_variant(&mut tx, &ctx, "soap", dec!(20)).await;
    let bundle = a_priced_variant(&mut tx, &ctx, "gift-set", dec!(0)).await;

    let cart = cart::create(
        &mut tx,
        &ctx,
        cart::NewCart {
            customer_id: None,
            email: None,
            currency_code: lira(),
            region_id: None,
            sales_channel_id: None,
            basket_id: None,
            expires_at: None,
            metadata: None,
        },
    )
    .await
    .expect("a cart");

    let lines = cart::add_bundle(
        &mut tx,
        &ctx,
        cart.id,
        AddBundle {
            variant_id: bundle,
            quantity: 2,
            is_tax_inclusive: false,
            components: vec![
                AddBundleComponent {
                    variant_id: candle,
                    quantity: 1,
                    unit_price: money(dec!(27)),
                },
                AddBundleComponent {
                    variant_id: soap,
                    quantity: 2,
                    unit_price: money(dec!(18)),
                },
            ],
        },
    )
    .await
    .expect("a bundle to be added");

    assert_eq!(lines.len(), 3, "one parent and two components");
    let parent = &lines[0];
    assert_eq!(parent.unit_price, dec!(0), "the parent carries no price");
    assert!(parent.parent_line_item_id.is_none());

    let children = &lines[1..];
    for child in children {
        assert_eq!(child.parent_line_item_id, Some(parent.id));
    }

    // Each child's `unit_price` is already the allocated share for every
    // component unit one bundle needs (`AddBundleComponent::quantity` fed
    // into that at resolution); the cart line's own `quantity` is how many
    // bundles, so the line total is `unit_price * bundle quantity` only —
    // not multiplied a second time by the component's own quantity.
    let child_total: Decimal = children
        .iter()
        .map(|c| c.unit_price * Decimal::from(c.quantity))
        .sum();
    assert_eq!(child_total, dec!(90), "(27 + 18) * 2 bundles");

    // Adding the bundle a second time is a quantity change on each line, the
    // same as an ordinary variant.
    let again = cart::add_bundle(
        &mut tx,
        &ctx,
        cart.id,
        AddBundle {
            variant_id: bundle,
            quantity: 1,
            is_tax_inclusive: false,
            components: vec![
                AddBundleComponent {
                    variant_id: candle,
                    quantity: 1,
                    unit_price: money(dec!(27)),
                },
                AddBundleComponent {
                    variant_id: soap,
                    quantity: 2,
                    unit_price: money(dec!(18)),
                },
            ],
        },
    )
    .await
    .expect("the bundle to be added again");
    assert_eq!(again[0].quantity, 3, "two plus one more bundle");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_bundle_component_returns_at_its_allocated_share_not_an_equal_split() -> tezgah::Result<()>
{
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    // Two components allocated an uneven share of the bundle price: 70 and
    // 30. If a return ever valued a component as an equal split of the
    // total, it would credit 50 for each instead.
    let cart_parent = tezgah::id::LineItemId::new();
    let cart_expensive = tezgah::id::LineItemId::new();
    let cart_cheap = tezgah::id::LineItemId::new();

    let new = NewOrder {
        email: Some("shopper@example.com".into()),
        shipping_address: Some(OrderAddress {
            address_1: Some("1 Example Street".into()),
            city: Some("Istanbul".into()),
            country_code: Some("TR".into()),
            ..OrderAddress::default()
        }),
        lines: vec![
            NewOrderLine {
                selling_plan_id: None,
                requires_shipping: false,
                reserved_for: Some(cart_parent),
                ..NewOrderLine::of("A gift set", 1, money(dec!(0)))
            },
            NewOrderLine {
                selling_plan_id: None,
                reserved_for: Some(cart_expensive),
                parent_cart_line: Some(cart_parent),
                ..NewOrderLine::of("A candle", 1, money(dec!(70)))
            },
            NewOrderLine {
                selling_plan_id: None,
                reserved_for: Some(cart_cheap),
                parent_cart_line: Some(cart_parent),
                ..NewOrderLine::of("A bar of soap", 1, money(dec!(30)))
            },
        ],
        ..NewOrder::of(lira())
    };

    let placed = order::create(&mut tx, &ctx, new).await?;
    let placed_lines = order::line_items(&mut tx, &ctx, placed.id).await?;

    let parent = placed_lines
        .iter()
        .find(|l| l.title == "A gift set")
        .expect("the parent line");
    let expensive = placed_lines
        .iter()
        .find(|l| l.title == "A candle")
        .expect("the expensive component");
    let cheap = placed_lines
        .iter()
        .find(|l| l.title == "A bar of soap")
        .expect("the cheap component");

    assert_eq!(expensive.parent_line_item_id, Some(parent.id));
    assert_eq!(cheap.parent_line_item_id, Some(parent.id));
    assert_eq!(expensive.unit_price, dec!(70));
    assert_eq!(cheap.unit_price, dec!(30));

    let asked = order::request_return(
        &mut tx,
        &ctx,
        placed.id,
        None,
        vec![ReturnLine {
            order_line_item_id: expensive.id,
            quantity: 1,
            return_reason_id: None,
            note: None,
        }],
    )
    .await?;
    assert_eq!(asked.order_id, placed.id);

    let items = order::items(&mut tx, &ctx, placed.id, placed.version).await?;
    let returned = items
        .iter()
        .find(|i| i.order_line_item_id == expensive.id)
        .expect("the returned item");
    assert_eq!(
        returned.unit_price,
        Some(dec!(70)),
        "the component's own allocated price, not an equal split of the bundle"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}

/// The database refuses a bundle listing itself as its own component, even
/// for an insert that goes around `pricing::add_bundle_component` entirely.
#[tokio::test]
async fn a_bundle_cannot_be_inserted_as_its_own_component() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let bundle = a_priced_variant(&mut tx, &ctx, "self-set", dec!(0)).await;

    let refused = sqlx::query(
        "insert into product_bundle_component
             (id, scope, bundle_variant_id, component_variant_id, quantity, sort_order)
         values ($1, $2, $3, $3, 1, 0)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.here.0)
    .bind(bundle.as_uuid())
    .execute(&mut *tx)
    .await;

    assert!(
        refused.is_err(),
        "a row naming the same variant on both sides should be rejected by the database"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Two bundles sharing a component get one line each, and a return of one
/// bundle never touches the other's line.
#[tokio::test]
async fn two_bundles_sharing_a_component_keep_separate_lines() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let honey = a_priced_variant(&mut tx, &ctx, "honey", dec!(10)).await;
    let starter = a_priced_variant(&mut tx, &ctx, "starter-kit", dec!(0)).await;
    let gift = a_priced_variant(&mut tx, &ctx, "gift-set", dec!(0)).await;

    let cart = cart::create(&mut tx, &ctx, cart::NewCart::guest(lira()))
        .await
        .expect("a cart");

    let starter_lines = cart::add_bundle(
        &mut tx,
        &ctx,
        cart.id,
        AddBundle {
            variant_id: starter,
            quantity: 1,
            is_tax_inclusive: false,
            components: vec![AddBundleComponent {
                variant_id: honey,
                quantity: 1,
                unit_price: money(dec!(10)),
            }],
        },
    )
    .await
    .expect("the starter kit to be added");

    let gift_lines = cart::add_bundle(
        &mut tx,
        &ctx,
        cart.id,
        AddBundle {
            variant_id: gift,
            quantity: 1,
            is_tax_inclusive: false,
            components: vec![AddBundleComponent {
                variant_id: honey,
                quantity: 1,
                unit_price: money(dec!(10)),
            }],
        },
    )
    .await
    .expect("the gift set to be added, not merged into the starter kit's honey line");

    let starter_honey = &starter_lines[1];
    let gift_honey = &gift_lines[1];
    assert_ne!(
        starter_honey.id, gift_honey.id,
        "each bundle's honey line is its own row"
    );
    assert_eq!(starter_honey.parent_line_item_id, Some(starter_lines[0].id));
    assert_eq!(gift_honey.parent_line_item_id, Some(gift_lines[0].id));

    // The same variant bought loose does not merge into either bundle's line.
    let standalone = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: honey,
            quantity: 1,
            unit_price: money(dec!(10)),
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await
    .expect("a loose jar to be its own line");
    assert!(standalone.parent_line_item_id.is_none());
    assert_ne!(standalone.id, starter_honey.id);
    assert_ne!(standalone.id, gift_honey.id);

    assert_eq!(
        line_count(&mut tx, cart.id).await,
        5,
        "two parents, two bundle-component lines and one standalone line"
    );

    // Returning the starter kit's honey does not touch the gift set's line.
    cart::remove_line(&mut tx, &ctx, cart.id, starter_honey.id)
        .await
        .expect("to remove the starter kit's component");

    let remaining_gift = cart::lines(&mut tx, &ctx, cart.id)
        .await
        .expect("the cart's lines")
        .into_iter()
        .find(|l| l.id == gift_honey.id)
        .expect("the gift set's honey line to be untouched");
    assert_eq!(remaining_gift.quantity, gift_honey.quantity);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
