mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::cart::{
    self, AddBundle, AddBundleComponent, AddLine, CartAddress, NewCart, NewShippingMethod,
    TotalsLine, TotalsShipping,
};
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::customer::{self, NewCustomer};
use tezgah::id::VariantId;
use tezgah::inventory;
use tezgah::money::{Currency, Money};
use tezgah::page::Paging;
use tezgah::ports::{Ctx, Tx};
use tezgah::subscription;

fn lira() -> tezgah::Result<Currency> {
    Currency::parse("TRY")
}

fn money(amount: Decimal) -> tezgah::Result<Money> {
    Ok(Money::new(amount, lira()?))
}

async fn a_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, handle: &str) -> tezgah::Result<VariantId> {
    let product = catalogue::create_product(
        tx,
        ctx,
        NewProduct {
            handle: handle.into(),
            title: format!("A {handle}"),
            ..NewProduct::default()
        },
    )
    .await?;

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
    .await?;

    Ok(variant.id)
}

#[tokio::test]
async fn adding_the_same_variant_twice_leaves_one_line() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "kettle").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;

    let first = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 2,
            unit_price: money(dec!(19.99))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let again = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 3,
            unit_price: money(dec!(19.99))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    assert_eq!(first.id, again.id);
    assert_eq!(again.quantity, 5);

    let lines = cart::lines(&mut tx, &ctx, cart.id).await?;
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].product_title, "A kettle");
    assert_eq!(lines[0].variant_sku.as_deref(), Some("kettle-1"));

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

async fn a_plan(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
) -> tezgah::Result<tezgah::id::SellingPlanId> {
    let group = subscription::create_plan_group(
        tx,
        ctx,
        subscription::NewPlanGroup {
            name: "Monthly".into(),
            ..subscription::NewPlanGroup::default()
        },
    )
    .await?;

    let plan = subscription::create_plan(
        tx,
        ctx,
        group.id,
        subscription::NewPlan {
            name: "Every month".into(),
            billing_interval_unit: "month".into(),
            billing_interval_count: 1,
            ..subscription::NewPlan::default()
        },
    )
    .await?;

    subscription::attach_variant(tx, ctx, plan.id, variant).await?;

    Ok(plan.id)
}

/// #139: `add_line` used to merge on `(cart_id, variant_id)` alone, so a
/// subscription line and a one-off line for the same variant collapsed into
/// one — and whichever add happened second decided the surviving quantity.
#[tokio::test]
async fn a_subscription_line_and_a_one_off_line_stay_separate() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "coffee").await?;
    let plan = a_plan(&mut tx, &ctx, variant).await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;

    let once = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let subscribed = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    assert_ne!(once.id, subscribed.id, "two lines, not one");

    let again_once = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 2,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;
    let again_subscribed = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 3,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    assert_eq!(again_once.id, once.id);
    assert_eq!(
        again_once.quantity, 3,
        "the one-off line kept its own count"
    );
    assert_eq!(again_subscribed.id, subscribed.id);
    assert_eq!(
        again_subscribed.quantity, 4,
        "the subscription line kept its own count"
    );

    let lines = cart::lines(&mut tx, &ctx, cart.id).await?;
    assert_eq!(lines.len(), 2);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// #139: a guest cart's subscription line must not merge into the customer's
/// existing one-off line for the same variant when the two carts combine.
#[tokio::test]
async fn merging_carts_keeps_a_subscription_line_off_a_one_off_line() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "coffee").await?;
    let plan = a_plan(&mut tx, &ctx, variant).await?;

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("dana@example.com")).await?;
    let theirs = cart::create(&mut tx, &ctx, NewCart::of(who.id, lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        theirs.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        guest.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    let merged = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    assert_eq!(merged.id, theirs.id);

    let lines = cart::lines(&mut tx, &ctx, merged.id).await?;
    assert_eq!(
        lines.len(),
        2,
        "the subscription line did not merge into the one-off line"
    );

    let one_off = lines
        .iter()
        .find(|line| line.selling_plan_id.is_none())
        .expect("the one-off line survived");
    assert_eq!(one_off.quantity, 1);

    let on_plan = lines
        .iter()
        .find(|line| line.selling_plan_id == Some(plan))
        .expect("the subscription line survived, still carrying its plan");
    assert_eq!(on_plan.quantity, 1);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_quantity_of_zero_takes_the_line_away() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "mug").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let line = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(5))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let raised = cart::update_line(&mut tx, &ctx, cart.id, line.id, 4).await?;
    assert_eq!(raised.map(|item| item.quantity), Some(4));

    assert!(
        cart::update_line(&mut tx, &ctx, cart.id, line.id, 0)
            .await?
            .is_none()
    );
    assert!(cart::lines(&mut tx, &ctx, cart.id).await?.is_empty());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_price_in_another_currency_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "plate").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let wrong = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: Money::new(dec!(5), Currency::parse("EUR")?),
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await;
    assert!(wrong.is_err());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn the_totals_of_a_real_cart_add_up() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let variant = a_variant(&mut tx, &ctx, "pan").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 3,
            unit_price: money(dec!(19.99))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;
    cart::set_shipping_method(
        &mut tx,
        &ctx,
        cart.id,
        NewShippingMethod {
            shipping_option_id: None,
            name: "Standard".into(),
            description: None,
            amount: money(dec!(15))?,
            is_tax_inclusive: false,
            data: None,
        },
    )
    .await?;

    let totals = cart::totals(&mut tx, &ctx, cart.id).await?;
    assert_eq!(totals.subtotal.amount, dec!(59.97));
    assert_eq!(totals.shipping.amount, dec!(15.00));
    assert_eq!(
        totals.total.amount,
        totals.subtotal.amount - totals.discount.amount
            + totals.shipping.amount
            + totals.tax.amount
    );

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn every_arrangement_of_parts_adds_up_to_the_total() -> tezgah::Result<()> {
    let currency = lira()?;
    let scenarios: [(Vec<TotalsLine>, Vec<TotalsShipping>); 4] = [
        (vec![], vec![]),
        (
            vec![TotalsLine {
                quantity: 3,
                unit_price: dec!(7.77),
                is_tax_inclusive: false,
                discount: dec!(2.5),
                tax_rate: dec!(18),
            }],
            vec![TotalsShipping {
                amount: dec!(24.9),
                is_tax_inclusive: false,
                discount: dec!(0),
                tax_rate: dec!(18),
            }],
        ),
        (
            vec![
                TotalsLine {
                    quantity: 1,
                    unit_price: dec!(118),
                    is_tax_inclusive: true,
                    discount: dec!(11.8),
                    tax_rate: dec!(18),
                },
                TotalsLine {
                    quantity: 11,
                    unit_price: dec!(0.07),
                    is_tax_inclusive: false,
                    discount: dec!(0),
                    tax_rate: dec!(1),
                },
            ],
            vec![],
        ),
        (
            vec![TotalsLine {
                quantity: 2,
                unit_price: dec!(1000000.123456),
                is_tax_inclusive: false,
                discount: dec!(333333.33),
                tax_rate: dec!(20),
            }],
            vec![TotalsShipping {
                amount: dec!(0),
                is_tax_inclusive: true,
                discount: dec!(0),
                tax_rate: dec!(0),
            }],
        ),
    ];

    for (lines, shipping) in scenarios {
        let totals = cart::compute(&lines, &shipping, currency, 2)?;
        assert_eq!(
            totals.total.amount,
            totals.subtotal.amount - totals.discount.amount
                + totals.shipping.amount
                + totals.tax.amount
        );
        assert!(!totals.subtotal.is_negative());
        assert!(!totals.tax.is_negative());
    }

    Ok(())
}

#[tokio::test]
async fn a_guest_cart_becomes_the_customers_and_merges_with_theirs() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let shared = a_variant(&mut tx, &ctx, "lamp").await?;
    let only_guest = a_variant(&mut tx, &ctx, "shade").await?;

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("hal@example.com")).await?;
    let theirs = cart::create(&mut tx, &ctx, NewCart::of(who.id, lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        theirs.id,
        AddLine {
            variant_id: shared,
            quantity: 1,
            unit_price: money(dec!(40))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    for (variant, quantity) in [(shared, 2), (only_guest, 5)] {
        cart::add_line(
            &mut tx,
            &ctx,
            guest.id,
            AddLine {
                variant_id: variant,
                quantity,
                unit_price: money(dec!(40))?,
                is_tax_inclusive: false,
                selling_plan_id: None,
            },
        )
        .await?;
    }

    let merged = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    assert_eq!(merged.id, theirs.id);
    assert_eq!(merged.customer_id, Some(who.id));

    let lines = cart::lines(&mut tx, &ctx, merged.id).await?;
    assert_eq!(lines.len(), 2);
    let quantity_of = |wanted: VariantId| {
        lines
            .iter()
            .find(|line| line.variant_id == Some(wanted))
            .map(|line| line.quantity)
    };
    assert_eq!(quantity_of(shared), Some(3));
    assert_eq!(quantity_of(only_guest), Some(5));

    assert!(cart::get(&mut tx, &ctx, guest.id).await.is_err());
    assert!(shop.host.emitted("cart.merged"));

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_guest_cart_with_no_cart_to_merge_into_is_simply_claimed() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "clock").await?;
    let who = customer::create(&mut tx, &ctx, NewCustomer::account("ida@example.com")).await?;
    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        guest.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(12))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let claimed = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    assert_eq!(claimed.id, guest.id);
    assert_eq!(claimed.customer_id, Some(who.id));
    assert_eq!(claimed.email.as_deref(), Some("ida@example.com"));
    assert_eq!(cart::lines(&mut tx, &ctx, guest.id).await?.len(), 1);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn addresses_and_an_email_are_kept_on_the_cart() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let with_email = cart::set_email(&mut tx, &ctx, cart.id, " Jo@Example.com ").await?;
    assert_eq!(with_email.email.as_deref(), Some("jo@example.com"));
    assert!(
        cart::set_email(&mut tx, &ctx, cart.id, "not-an-address")
            .await
            .is_err()
    );

    let addressed = cart::set_addresses(
        &mut tx,
        &ctx,
        cart.id,
        Some(CartAddress {
            address_1: Some("5 Fifth Street".into()),
            city: Some("Ankara".into()),
            country_code: Some("tr".into()),
            ..CartAddress::default()
        }),
        None,
    )
    .await?;
    assert!(addressed.shipping_address_id.is_some());
    assert!(addressed.billing_address_id.is_none());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_that_ran_out_of_time_is_swept_away() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let now = chrono::Utc::now();
    let stale = cart::create(
        &mut tx,
        &ctx,
        NewCart {
            expires_at: Some(now - chrono::Duration::hours(1)),
            ..NewCart::guest(lira()?)
        },
    )
    .await?;
    let live = cart::create(
        &mut tx,
        &ctx,
        NewCart {
            expires_at: Some(now + chrono::Duration::hours(1)),
            ..NewCart::guest(lira()?)
        },
    )
    .await?;

    let gone = cart::expire(&mut tx, &ctx, now).await?;
    assert_eq!(gone, vec![stale.id]);
    assert!(cart::get(&mut tx, &ctx, stale.id).await.is_err());
    assert!(cart::get(&mut tx, &ctx, live.id).await.is_ok());
    assert!(shop.host.emitted("cart.expired"));

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_swept_cart_gives_back_what_it_reserved() -> tezgah::Result<()> {
    let shop = Shop::open().await;
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
    .await?;
    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", uuid::Uuid::now_v7())),
            title: Some("a mug".into()),
            requires_shipping: true,
        },
    )
    .await?;
    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 5, 0).await?;

    let variant = a_variant(&mut tx, &ctx, "mug").await?;
    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1).await?;

    let now = chrono::Utc::now();
    let cart = cart::create(
        &mut tx,
        &ctx,
        NewCart {
            expires_at: Some(now - chrono::Duration::hours(1)),
            ..NewCart::guest(lira()?)
        },
    )
    .await?;
    let line = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 2,
            unit_price: money(dec!(19.99))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    // Checkout would reserve the line's stock; simulated here so the hold
    // exists for `expire` to give back — reproducing exactly what a cart
    // abandoned mid-checkout leaves behind.
    inventory::reserve(
        &mut tx,
        &ctx,
        item.id,
        location.id,
        2,
        Some(line.id),
        false,
        None,
    )
    .await?;

    let level = inventory::level(&mut tx, &ctx, item.id, location.id).await?;
    assert_eq!((level.reserved_quantity, level.available_quantity), (2, 3));

    let gone = cart::expire(&mut tx, &ctx, now).await?;
    assert_eq!(gone, vec![cart.id]);

    let level = inventory::level(&mut tx, &ctx, item.id, location.id).await?;
    assert_eq!((level.reserved_quantity, level.available_quantity), (0, 5));
    assert!(shop.host.emitted("stock.released"));

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_cannot_reach_the_cart() -> tezgah::Result<()> {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let ctx = shop.ctx();
    let variant = a_variant(&mut mine, &ctx, "tray").await?;
    let cart = cart::create(&mut mine, &ctx, NewCart::guest(lira()?)).await?;
    cart::add_line(
        &mut mine,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(9))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;
    mine.commit().await?;

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let elsewhere = shop.theirs();
    assert!(cart::get(&mut theirs, &elsewhere, cart.id).await.is_err());
    assert!(
        cart::totals(&mut theirs, &elsewhere, cart.id)
            .await
            .is_err()
    );
    assert!(
        cart::list(&mut theirs, &elsewhere, None, Paging::first(10))
            .await?
            .is_empty()
    );
    theirs.rollback().await.ok();

    shop.close().await;
    Ok(())
}

/// Marks a variant as not needing a shipping address, independently of
/// whether the shop tracks its stock at all — the fact belongs to the
/// catalogue, not to the inventory link.
async fn digital(tx: &mut Tx<'_>, ctx: &Ctx<'_>, variant: VariantId) -> tezgah::Result<()> {
    catalogue::update_variant(
        tx,
        ctx,
        variant,
        catalogue::VariantPatch {
            requires_shipping: Some(false),
            ..catalogue::VariantPatch::default()
        },
    )
    .await?;
    Ok(())
}

/// What a line has to be sent, and where it is supplied from.
///
/// The value is the inventory item's rather than the line's own: a variant
/// nothing is counted for is a variant nothing is posted for, and that is
/// exactly what a gift card or a download is.
async fn stocked(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
    requires_shipping: bool,
) -> tezgah::Result<()> {
    let item = inventory::create_inventory_item(
        tx,
        ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("stock-{}", uuid::Uuid::now_v7().simple())),
            title: None,
            requires_shipping,
        },
    )
    .await?;

    inventory::attach_inventory_item(tx, ctx, variant, item.id, 1).await?;
    Ok(())
}

#[tokio::test]
async fn a_variant_marked_non_physical_does_not_ask_to_be_shipped() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let download = a_variant(&mut tx, &ctx, "album").await?;
    digital(&mut tx, &ctx, download).await?;
    let kettle = a_variant(&mut tx, &ctx, "kettle").await?;
    stocked(&mut tx, &ctx, kettle, true).await?;

    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let digital = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: download,
            quantity: 1,
            unit_price: money(dec!(30))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;
    let physical = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: kettle,
            quantity: 1,
            unit_price: money(dec!(50))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    assert!(
        !digital.requires_shipping,
        "a variant the catalogue marked non-physical still asked to be posted somewhere"
    );
    assert!(physical.requires_shipping);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// #115: a shop that never links an `inventory_item` — it does not track
/// stock at all — must not have every one of its variants read as digital.
/// Physical is the catalogue's default, independent of whether anybody is
/// counting.
#[tokio::test]
async fn a_shop_that_does_not_track_stock_still_ships_a_physical_variant() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let kettle = a_variant(&mut tx, &ctx, "untracked-kettle").await?;
    let album = a_variant(&mut tx, &ctx, "untracked-album").await?;
    digital(&mut tx, &ctx, album).await?;

    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let physical = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: kettle,
            quantity: 1,
            unit_price: money(dec!(50))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;
    let digital_line = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: album,
            quantity: 1,
            unit_price: money(dec!(30))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    assert!(
        physical.requires_shipping,
        "a shop with no inventory tracking still sells physical goods"
    );
    assert!(!digital_line.requires_shipping);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_inventory_item_that_ships_nothing_makes_a_line_that_ships_nothing() -> tezgah::Result<()>
{
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let card = a_variant(&mut tx, &ctx, "gift-card").await?;
    stocked(&mut tx, &ctx, card, false).await?;

    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let line = cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: card,
            quantity: 1,
            unit_price: money(dec!(100))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    assert!(!line.requires_shipping);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_merge_does_not_spread_a_line_that_ships_nowhere() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let download = a_variant(&mut tx, &ctx, "audiobook").await?;
    digital(&mut tx, &ctx, download).await?;
    let kettle = a_variant(&mut tx, &ctx, "pan").await?;
    stocked(&mut tx, &ctx, kettle, true).await?;

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("nils@example.com")).await?;
    let mine = cart::create(&mut tx, &ctx, NewCart::of(who.id, lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        mine.id,
        AddLine {
            variant_id: kettle,
            quantity: 1,
            unit_price: money(dec!(50))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        guest.id,
        AddLine {
            variant_id: download,
            quantity: 1,
            unit_price: money(dec!(20))?,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let merged = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    let lines = cart::lines(&mut tx, &ctx, merged.id).await?;
    assert_eq!(lines.len(), 2);

    for line in lines {
        let ships = line.variant_id == Some(kettle);
        assert_eq!(
            line.requires_shipping, ships,
            "the merge carried the wrong answer for {}",
            line.product_title
        );
    }

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_that_ships_nothing_is_supplied_where_it_is_billed() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let cart = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    cart::set_addresses(
        &mut tx,
        &ctx,
        cart.id,
        Some(CartAddress {
            country_code: Some("DE".into()),
            ..CartAddress::default()
        }),
        Some(CartAddress {
            country_code: Some("FR".into()),
            ..CartAddress::default()
        }),
    )
    .await?;

    let parcel = cart::delivery(&mut tx, &ctx, cart.id)
        .await?
        .expect("a country");
    let supply = cart::place_of_supply(&mut tx, &ctx, cart.id)
        .await?
        .expect("a country");

    assert_eq!(parcel.country_code, "DE");
    assert_eq!(supply.country_code, "FR");

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// A cart rounds by the exponent its currency actually has. Two decimals is
/// wrong for JPY, wrong for KWD, and was what both used to get.
async fn totals_in(code: &str, exponent: i16, unit: Decimal) -> tezgah::Result<Decimal> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    common::a_currency(&mut tx, shop.here, code, exponent).await;

    let money = Money::new(unit, Currency::parse(code)?);
    let variant = a_variant(&mut tx, &ctx, "bowl").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(Currency::parse(code)?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 3,
            unit_price: money,
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let totals = cart::totals(&mut tx, &ctx, cart.id).await?;
    let subtotal = totals.subtotal.amount;

    tx.rollback().await.ok();
    shop.close().await;
    Ok(subtotal)
}

#[tokio::test]
async fn a_cart_in_yen_keeps_no_decimals() -> tezgah::Result<()> {
    assert_eq!(totals_in("JPY", 0, dec!(333.333)).await?, dec!(1000));
    Ok(())
}

#[tokio::test]
async fn a_cart_in_dinars_keeps_three() -> tezgah::Result<()> {
    assert_eq!(totals_in("KWD", 3, dec!(1.23456)).await?, dec!(3.704));
    Ok(())
}

#[tokio::test]
async fn a_currency_this_shop_has_not_configured_is_not_two_decimals() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = a_variant(&mut tx, &ctx, "jug").await?;
    let cart = cart::create(&mut tx, &ctx, NewCart::guest(Currency::parse("NOK")?)).await?;
    cart::add_line(
        &mut tx,
        &ctx,
        cart.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: Money::new(dec!(9.999), Currency::parse("NOK")?),
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await?;

    let refused = cart::totals(&mut tx, &ctx, cart.id).await.unwrap_err();
    assert!(refused.is_not_found(), "{refused} was not a not_found");
    assert!(!refused.is_internal());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// The columns a merge is allowed to change: a new identity, a new parent
/// (remapped rather than copied), and timestamps a fresh row gets its own of.
/// `quantity` is excluded too — a merge that lands on an existing line adds
/// to it rather than copying it. Everything else on `cart_line_item` must
/// travel from the source line untouched, whatever it is called.
const MERGE_MAY_CHANGE: &[&str] = &[
    "id",
    "scope",
    "cart_id",
    "parent_line_item_id",
    "quantity",
    "created_at",
    "updated_at",
];

/// Reads `cart_line_item`'s own columns rather than a list kept by hand, so a
/// column added to the table tomorrow is asked about the day it is added.
async fn cart_line_item_columns_to_check(tx: &mut Tx<'_>) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "select column_name from information_schema.columns
         where table_schema = 'public' and table_name = 'cart_line_item'
           and column_name <> all($1)
         order by column_name",
    )
    .bind(MERGE_MAY_CHANGE)
    .fetch_all(&mut **tx)
    .await
    .expect("to read cart_line_item's columns")
}

async fn cart_line_item_as_json(tx: &mut Tx<'_>, id: uuid::Uuid) -> serde_json::Value {
    sqlx::query_scalar::<_, serde_json::Value>(
        "select to_jsonb(cart_line_item) from cart_line_item where id = $1",
    )
    .bind(id)
    .fetch_one(&mut **tx)
    .await
    .expect("the line to still be there")
}

/// Fails naming the first column where `merged` does not carry `source`'s
/// value, over the columns a merge has no business changing.
fn assert_merge_carried_every_column(
    columns: &[String],
    source: &serde_json::Value,
    merged: &serde_json::Value,
) {
    for column in columns {
        assert_eq!(
            source.get(column),
            merged.get(column),
            "a merge dropped `{column}`"
        );
    }
}

/// #140: proves the catalogue-driven check above actually catches a dropped
/// column, rather than passing by construction. `variant_sku` is deleted from
/// a stand-in "merged" row here — no migration or source file is touched —
/// and the assertion above must fail on exactly that column.
#[test]
#[should_panic(expected = "a merge dropped `variant_sku`")]
fn the_catalogue_driven_merge_check_catches_a_dropped_column() {
    let columns = vec!["variant_sku".to_string(), "product_title".to_string()];

    let source = serde_json::json!({
        "variant_sku": "kettle-1",
        "product_title": "A kettle",
    });

    // A copy that forgot to name `variant_sku`, the way `transfer_to_customer`
    // forgot `parent_line_item_id` in #136 and `selling_plan_id` in #139.
    let merged = serde_json::json!({
        "product_title": "A kettle",
    });

    assert_merge_carried_every_column(&columns, &source, &merged);
}

/// #140: `cart::transfer_to_customer` used to name each column by hand and
/// has twice dropped one silently. This reads `cart_line_item`'s own columns
/// so the same mistake cannot happen a third time without the test noticing
/// the day the column is added, not the day someone goes looking for it.
#[tokio::test]
async fn merging_carts_carries_every_cart_line_item_column() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let columns = cart_line_item_columns_to_check(&mut tx).await;
    assert!(
        !columns.is_empty(),
        "the exception list ate the whole table"
    );

    let variant = a_variant(&mut tx, &ctx, "teapot").await?;
    let plan = a_plan(&mut tx, &ctx, variant).await?;

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("nur@example.com")).await?;
    // A cart of their own already exists, so the merge path runs rather than
    // the plain hand-off `set_customer` takes when there is nothing to merge.
    cart::create(&mut tx, &ctx, NewCart::of(who.id, lira()?)).await?;

    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let added = cart::add_line(
        &mut tx,
        &ctx,
        guest.id,
        AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: money(dec!(40))?,
            is_tax_inclusive: false,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    let source = cart_line_item_as_json(&mut tx, added.id.as_uuid()).await;

    let merged = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    let lines = cart::lines(&mut tx, &ctx, merged.id).await?;
    assert_eq!(lines.len(), 1);
    let merged_line = cart_line_item_as_json(&mut tx, lines[0].id.as_uuid()).await;

    assert_merge_carried_every_column(&columns, &source, &merged_line);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// #140 / #136: a bundle's child must still name its parent's *merged* row
/// after a guest cart's lines are copied into the customer's — the exact
/// regression `parent_line_item_id` being left off the column list caused.
#[tokio::test]
async fn merging_carts_keeps_a_bundle_childs_parent() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let box_ = a_variant(&mut tx, &ctx, "gift-box").await?;
    let candle = a_variant(&mut tx, &ctx, "wax-candle").await?;

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("aylin@example.com")).await?;
    cart::create(&mut tx, &ctx, NewCart::of(who.id, lira()?)).await?;

    let guest = cart::create(&mut tx, &ctx, NewCart::guest(lira()?)).await?;
    let bundle_lines = cart::add_bundle(
        &mut tx,
        &ctx,
        guest.id,
        AddBundle {
            variant_id: box_,
            quantity: 1,
            is_tax_inclusive: false,
            components: vec![AddBundleComponent {
                variant_id: candle,
                quantity: 1,
                unit_price: money(dec!(15))?,
            }],
        },
    )
    .await?;
    assert_eq!(bundle_lines.len(), 2);

    let merged = cart::transfer_to_customer(&mut tx, &ctx, guest.id, who.id).await?;
    let lines = cart::lines(&mut tx, &ctx, merged.id).await?;
    assert_eq!(lines.len(), 2);

    let parent = lines
        .iter()
        .find(|line| line.parent_line_item_id.is_none())
        .expect("the bundle's parent line survived the merge");
    let child = lines
        .iter()
        .find(|line| line.parent_line_item_id.is_some())
        .expect("the bundle's child line survived the merge");

    assert_eq!(
        child.parent_line_item_id,
        Some(parent.id),
        "the child lost track of its parent when the carts merged"
    );

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}
