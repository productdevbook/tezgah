mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::cart::{
    self, AddLine, CartAddress, NewCart, NewShippingMethod, TotalsLine, TotalsShipping,
};
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::customer::{self, NewCustomer};
use tezgah::id::VariantId;
use tezgah::inventory;
use tezgah::money::{Currency, Money};
use tezgah::page::Paging;
use tezgah::ports::{Ctx, Tx};

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
