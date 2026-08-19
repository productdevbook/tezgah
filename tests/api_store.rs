//! The storefront surface against a real Postgres.
//!
//! The two rules the surface exists to keep are the two worth testing: nothing
//! unpublished is visible, and nobody reaches somebody else's cart. The rest is
//! the paging ceiling, the shape of what comes in, and the scope boundary — all
//! of which have been wrong in a way that only shows up when it is run.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::Shop;
use rust_decimal_macros::dec;
use tezgah::api::store::{
    self, AddLineItem, CreateCart, ListCategories, ListOptions, ListPage, ListPaymentProviders,
    ListProducts, ListReturnReasons, ListVariants, StartPayment, StartPaymentSession,
};
use tezgah::cart;
use tezgah::catalogue::{
    self, CategoryTranslation, NewCategory, NewProduct, NewVariant, ProductTranslation,
};
use tezgah::fulfilment;
use tezgah::id::{CartId, CustomerId, PaymentCollectionId};
use tezgah::money::{Currency, Money};
use tezgah::order;
use tezgah::page::MAX_LIMIT;
use tezgah::payment;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, Host, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use tezgah::pricing::{self, NewPrice};
use tezgah::store::{self as domain_store, NewSalesChannel, NewStore};
use tezgah::subscription;
use uuid::Uuid;

/// A fresh publishable key's token, linked to no channel — the state every
/// shop starts in, and so one that sees whatever `channels: None` used to.
async fn any_token(tx: &mut Tx<'_>, ctx: &tezgah::ports::Ctx<'_>) -> String {
    domain_store::create_publishable_key(tx, ctx, "storefront")
        .await
        .expect("a token")
        .token
}

/// A shop's own settings row, the way a host's onboarding creates one.
async fn a_store_row(tx: &mut Tx<'_>, ctx: &tezgah::ports::Ctx<'_>) {
    domain_store::create_store(
        tx,
        ctx,
        NewStore {
            name: "Test shop".into(),
            default_currency_code: Currency::parse("TRY").expect("a currency"),
            supported_currency_codes: Vec::new(),
            supported_locales: Vec::new(),
            default_region_id: None,
            default_sales_channel_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a store row");
}

/// A host that answers ownership the way a real one would, and says yes to
/// everything else.
///
/// It grants a cart whose owner is unknown on purpose: the domain call asks
/// that way, so a refusal here would prove nothing about the surface. What the
/// test is watching for is the second question, the one the surface asks with
/// the owner it has just read.
#[derive(Debug, Default)]
struct OnlyOwner;

impl Authorizer for OnlyOwner {
    fn authorize(&self, actor: &Actor, _: Action, resource: &Resource) -> tezgah::Result<Permit> {
        let owner = match resource {
            Resource::Cart { customer, .. }
            | Resource::Order { customer, .. }
            | Resource::Payment { customer, .. } => *customer,
            _ => None,
        };

        match (owner, actor) {
            (None, _) => Ok(Permit::granted()),
            (Some(owner), Actor::Customer { id }) if owner == *id => Ok(Permit::granted()),
            (Some(_), _) => Err(tezgah::Error::denied()),
        }
    }
}

impl Clock for OnlyOwner {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for OnlyOwner {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for OnlyOwner {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for OnlyOwner {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

async fn a_customer(tx: &mut Tx<'_>, shop: &Shop, email: &str) -> tezgah::Result<CustomerId> {
    let made = tezgah::customer::create(
        tx,
        &shop.ctx(),
        tezgah::customer::NewCustomer {
            email: Some(email.into()),
            first_name: None,
            last_name: None,
            phone: None,
            company_name: None,
            has_account: true,
            metadata: None,
        },
    )
    .await?;

    Ok(made.id)
}

fn draft(handle: &str) -> NewProduct {
    NewProduct {
        handle: handle.into(),
        title: format!("A {handle}"),
        ..NewProduct::default()
    }
}

#[tokio::test]
async fn a_draft_is_not_here_and_a_published_one_is() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim")).await?;
    let token = any_token(&mut tx, &ctx).await;

    let hidden = store::get_product(&mut tx, &ctx, &token, "kilim", None)
        .await
        .expect_err("a draft is not on the storefront");
    assert!(
        hidden.is_not_found(),
        "a draft answers not_found rather than denied, so a stranger learns nothing"
    );
    assert!(!hidden.is_denied());

    let listed = store::list_products(&mut tx, &ctx, &token, ListProducts::default()).await?;
    assert!(listed.is_empty(), "a draft is not listed either");

    catalogue::publish_product(&mut tx, &ctx, made.id).await?;

    let shown = store::get_product(&mut tx, &ctx, &token, "kilim", None).await?;
    assert_eq!(shown.handle, "kilim");

    let listed = store::list_products(&mut tx, &ctx, &token, ListProducts::default()).await?;
    assert_eq!(listed.len(), 1);

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #188: `catalogue::localised` was reachable from the admin surface only, so
/// a Turkish storefront showed an English title until a host built its own
/// second call. `locale` is a parameter on the one route now, not a second
/// one, and a miss falls back to the shop's own title rather than refusing.
#[tokio::test]
async fn a_storefront_product_is_read_in_its_requested_locale_and_falls_back() -> tezgah::Result<()>
{
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let token = any_token(&mut tx, &ctx).await;

    let fallen_back = store::get_product(&mut tx, &ctx, &token, "kilim", Some("tr")).await?;
    assert_eq!(
        fallen_back.title, "A kilim",
        "no Turkish translation yet, so the shop's own title answers"
    );

    catalogue::put_translation(
        &mut tx,
        &ctx,
        made.id,
        ProductTranslation {
            product_id: made.id,
            locale: "tr".into(),
            title: "Bir kilim".into(),
            subtitle: None,
            description: None,
            handle: None,
        },
    )
    .await?;

    let translated = store::get_product(&mut tx, &ctx, &token, "kilim", Some("tr")).await?;
    assert_eq!(translated.title, "Bir kilim");

    let untranslated = store::get_product(&mut tx, &ctx, &token, "kilim", None).await?;
    assert_eq!(
        untranslated.title, "A kilim",
        "no locale asked for, so the shop's own title answers, translation or not"
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #188: a Turkish category listing with English product titles was the same
/// bug one page up from the product itself.
#[tokio::test]
async fn list_products_reads_the_requested_locale_too() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let token = any_token(&mut tx, &ctx).await;

    catalogue::put_translation(
        &mut tx,
        &ctx,
        made.id,
        ProductTranslation {
            product_id: made.id,
            locale: "tr".into(),
            title: "Bir kilim".into(),
            subtitle: None,
            description: None,
            handle: None,
        },
    )
    .await?;

    let listed = store::list_products(
        &mut tx,
        &ctx,
        &token,
        ListProducts {
            locale: Some("tr".into()),
            ..ListProducts::default()
        },
    )
    .await?;
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].title, "Bir kilim");

    let untranslated = store::list_products(&mut tx, &ctx, &token, ListProducts::default()).await?;
    assert_eq!(untranslated.items[0].title, "A kilim");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #188/#173: a browsable category read its locale as a query parameter on
/// its own route now, not a second `…/translations/{locale}` one.
#[tokio::test]
async fn a_storefront_category_is_read_in_its_requested_locale() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let rugs = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            name: "Rugs".into(),
            handle: "rugs".into(),
            is_active: Some(true),
            is_internal: Some(false),
            ..NewCategory::default()
        },
    )
    .await?;

    let fallen_back = store::get_product_category(&mut tx, &ctx, rugs.id, Some("tr")).await?;
    assert_eq!(fallen_back.name, "Rugs");

    catalogue::put_category_translation(
        &mut tx,
        &ctx,
        rugs.id,
        CategoryTranslation {
            category_id: rugs.id,
            locale: "tr".into(),
            name: "Halılar".into(),
            description: None,
        },
    )
    .await?;

    let translated = store::get_product_category(&mut tx, &ctx, rugs.id, Some("tr")).await?;
    assert_eq!(translated.name, "Halılar");

    let untranslated = store::get_product_category(&mut tx, &ctx, rugs.id, None).await?;
    assert_eq!(untranslated.name, "Rugs");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #192: a Turkish category listing with English names was the same bug #188
/// fixed for `list_products`, one layer up, sitting next to the category's own
/// page in any storefront.
#[tokio::test]
async fn list_product_categories_reads_the_requested_locale_too() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let rugs = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            name: "Rugs".into(),
            handle: "rugs".into(),
            is_active: Some(true),
            is_internal: Some(false),
            ..NewCategory::default()
        },
    )
    .await?;

    catalogue::put_category_translation(
        &mut tx,
        &ctx,
        rugs.id,
        CategoryTranslation {
            category_id: rugs.id,
            locale: "tr".into(),
            name: "Halılar".into(),
            description: None,
        },
    )
    .await?;

    let listed = store::list_product_categories(
        &mut tx,
        &ctx,
        ListCategories {
            locale: Some("tr".into()),
            ..ListCategories::default()
        },
    )
    .await?;
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].name, "Halılar");

    let untranslated =
        store::list_product_categories(&mut tx, &ctx, ListCategories::default()).await?;
    assert_eq!(untranslated.items[0].name, "Rugs");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #188/#173: a return reason reads its locale as a query parameter on its
/// own route now too.
#[tokio::test]
async fn a_storefront_return_reason_is_read_in_its_requested_locale() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let id = Uuid::now_v7();
    sqlx::query("insert into return_reason (id, scope, value, label) values ($1, $2, $3, $4)")
        .bind(id)
        .bind(shop.here.0)
        .bind(id.simple().to_string())
        .bind("Wrong size")
        .execute(&mut *tx)
        .await
        .expect("a return reason");

    let fallen_back = store::get_return_reason(&mut tx, &ctx, id, Some("tr")).await?;
    assert_eq!(fallen_back.label, "Wrong size");

    order::put_return_reason_translation(
        &mut tx,
        &ctx,
        id,
        order::ReturnReasonTranslation {
            return_reason_id: id,
            locale: "tr".into(),
            label: "Yanlış beden".into(),
            description: None,
        },
    )
    .await?;

    let translated = store::get_return_reason(&mut tx, &ctx, id, Some("tr")).await?;
    assert_eq!(translated.label, "Yanlış beden");

    let untranslated = store::get_return_reason(&mut tx, &ctx, id, None).await?;
    assert_eq!(untranslated.label, "Wrong size");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #192: the same one-list-route-behind-its-own-page bug as
/// `list_product_categories`, found by checking every other list route with a
/// locale on its single-row sibling.
#[tokio::test]
async fn list_return_reasons_reads_the_requested_locale_too() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let id = Uuid::now_v7();
    sqlx::query("insert into return_reason (id, scope, value, label) values ($1, $2, $3, $4)")
        .bind(id)
        .bind(shop.here.0)
        .bind(id.simple().to_string())
        .bind("Wrong size")
        .execute(&mut *tx)
        .await
        .expect("a return reason");

    order::put_return_reason_translation(
        &mut tx,
        &ctx,
        id,
        order::ReturnReasonTranslation {
            return_reason_id: id,
            locale: "tr".into(),
            label: "Yanlış beden".into(),
            description: None,
        },
    )
    .await?;

    let listed = store::list_return_reasons(
        &mut tx,
        &ctx,
        ListReturnReasons {
            locale: Some("tr".into()),
            ..ListReturnReasons::default()
        },
    )
    .await?;
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].label, "Yanlış beden");

    let untranslated =
        store::list_return_reasons(&mut tx, &ctx, ListReturnReasons::default()).await?;
    assert_eq!(untranslated.items[0].label, "Wrong size");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #188/#173: a shipping option's own list route takes the locale as a
/// parameter now, not a separate `…/translations/{locale}` route.
#[tokio::test]
async fn a_storefront_shipping_option_is_read_in_its_requested_locale() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = fulfilment::create_set(
        &mut tx,
        &ctx,
        fulfilment::NewFulfillmentSet {
            name: "Delivery".into(),
            kind: fulfilment::SetKind::Shipping,
        },
    )
    .await?;
    let zone = fulfilment::create_service_zone(
        &mut tx,
        &ctx,
        fulfilment::NewServiceZone {
            name: "Everywhere".into(),
            fulfillment_set_id: set.id,
        },
    )
    .await?;
    fulfilment::create_geo_zone(
        &mut tx,
        &ctx,
        fulfilment::NewGeoZone {
            kind: fulfilment::ZoneKind::Country,
            country_code: "TR".into(),
            province_code: None,
            city: None,
            postal_expression: None,
            service_zone_id: zone.id,
        },
    )
    .await?;
    let option = fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        fulfilment::NewShippingOption {
            name: "Next day".into(),
            price_type: fulfilment::PriceKind::Flat,
            service_zone_id: zone.id,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
            is_return: false,
            enabled_in_store: true,
        },
    )
    .await?;

    let token = any_token(&mut tx, &ctx).await;
    let opened = store::create_cart(
        &mut tx,
        &ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;

    let query = |locale: Option<&str>| store::ListShippingOptions {
        cart_id: opened.id,
        country_code: "TR".into(),
        province_code: None,
        city: None,
        postal_code: None,
        locale: locale.map(str::to_owned),
    };

    let fallen_back = store::list_shipping_options(&mut tx, &ctx, query(Some("tr"))).await?;
    assert_eq!(fallen_back.len(), 1);
    assert_eq!(
        fallen_back[0].name, "Next day",
        "no Turkish translation yet, so the shop's own name answers"
    );

    fulfilment::put_shipping_option_translation(
        &mut tx,
        &ctx,
        option.id,
        fulfilment::ShippingOptionTranslation {
            shipping_option_id: option.id,
            locale: "tr".into(),
            name: "Ertesi gün".into(),
        },
    )
    .await?;

    let translated = store::list_shipping_options(&mut tx, &ctx, query(Some("tr"))).await?;
    assert_eq!(translated[0].name, "Ertesi gün");

    let untranslated = store::list_shipping_options(&mut tx, &ctx, query(None)).await?;
    assert_eq!(untranslated[0].name, "Next day");

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_variant_of_a_draft_is_not_here_either() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kettle")).await?;
    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewVariant {
            title: "One size".into(),
            ..catalogue::NewVariant::default()
        },
    )
    .await?;

    let token = any_token(&mut tx, &ctx).await;

    assert!(
        store::get_variant(&mut tx, &ctx, &token, variant.id)
            .await
            .expect_err("its product is a draft")
            .is_not_found()
    );

    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let seen = store::get_variant(&mut tx, &ctx, &token, variant.id).await?;
    assert_eq!(seen.id, variant.id);

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A product sold in more than one colour: picking a variant shows that
/// variant's own photos, not the other colour's — and a variant nobody has
/// photographed specifically still shows something, the product's own.
#[tokio::test]
async fn a_variant_shows_its_own_images_from_the_storefront() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim")).await?;
    let red = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewVariant {
            title: "Red".into(),
            ..catalogue::NewVariant::default()
        },
    )
    .await?;
    let blue = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewVariant {
            title: "Blue".into(),
            ..catalogue::NewVariant::default()
        },
    )
    .await?;

    let shared = catalogue::add_image(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewImage {
            url: "https://example.com/shared.jpg".into(),
            ..catalogue::NewImage::default()
        },
    )
    .await?;
    let red_only = catalogue::add_image(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewImage {
            url: "https://example.com/red.jpg".into(),
            ..catalogue::NewImage::default()
        },
    )
    .await?;
    catalogue::attach_image_to_variant(&mut tx, &ctx, red.id, red_only.id).await?;

    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let token = any_token(&mut tx, &ctx).await;

    let seen_red = store::get_variant(&mut tx, &ctx, &token, red.id).await?;
    let red_ids: Vec<_> = seen_red.images.iter().map(|image| image.id).collect();
    assert_eq!(
        red_ids,
        vec![red_only.id],
        "the red variant shows only what was attached to it, not the shared gallery too"
    );

    let seen_blue = store::get_variant(&mut tx, &ctx, &token, blue.id).await?;
    let blue_ids: Vec<_> = seen_blue.images.iter().map(|image| image.id).collect();
    assert_eq!(
        blue_ids,
        vec![shared.id, red_only.id],
        "a variant nothing was attached to falls back to the product's whole gallery, \
         attached-elsewhere images included"
    );

    let listed = store::list_variants(
        &mut tx,
        &ctx,
        &token,
        ListVariants {
            product_id: made.id,
            after: None,
            limit: None,
        },
    )
    .await?;
    let by_id: std::collections::HashMap<_, _> = listed
        .items
        .into_iter()
        .map(|variant| (variant.id, variant.images.len()))
        .collect();
    assert_eq!(by_id.get(&red.id), Some(&1));
    assert_eq!(
        by_id.get(&blue.id),
        Some(&2),
        "the list falls back the same way get_variant does"
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_is_not_reached_by_somebody_else() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let host: Arc<OnlyOwner> = Arc::new(OnlyOwner);
    let mut tx = shop.begin().await;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let theirs = a_customer(&mut tx, &shop, "somebody@example.test").await?;

    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        host.as_ref() as &dyn Host,
    );
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: theirs.as_uuid(),
        },
        host.as_ref() as &dyn Host,
    );

    let token = any_token(&mut tx, &ctx).await;
    let held = store::create_cart(
        &mut tx,
        &ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;
    assert_eq!(held.customer_id, Some(mine));

    let read = store::get_cart(&mut tx, &ctx, held.id).await?;
    assert_eq!(read.id, held.id);

    let refused = store::get_cart(&mut tx, &stranger, held.id)
        .await
        .expect_err("somebody else's cart is not theirs to read");
    assert!(
        refused.is_denied(),
        "the surface handed the host the cart's owner and the host said no"
    );

    assert!(
        store::update_cart(&mut tx, &stranger, held.id, store::UpdateCart::default())
            .await
            .expect_err("nor to write")
            .is_denied()
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A cart, and a payment collection opened for it, for one shopper.
async fn a_cart_with_money_owed(
    tx: &mut Tx<'_>,
    ctx: &tezgah::ports::Ctx<'_>,
) -> tezgah::Result<(CartId, PaymentCollectionId)> {
    common::a_currency(tx, ctx.scope, "TRY", 2).await;
    let token = any_token(tx, ctx).await;

    let cart = store::create_cart(
        tx,
        ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;

    let collection =
        store::create_payment_collection(tx, ctx, StartPayment { cart_id: cart.id }).await?;

    Ok((cart.id, collection.id))
}

/// The exploit the surface used to allow: the cart in the body was the only
/// thing ownership was asked about, and the collection in the path was
/// anybody's.
#[tokio::test]
async fn a_session_is_not_opened_on_somebody_elses_collection() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    payment::register_provider(&mut tx, &shop.ctx(), "mock").await?;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let theirs = a_customer(&mut tx, &shop, "somebody@example.test").await?;

    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        shop.host.as_ref() as &dyn Host,
    );
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: theirs.as_uuid(),
        },
        shop.host.as_ref() as &dyn Host,
    );

    let (my_cart, _) = a_cart_with_money_owed(&mut tx, &ctx).await?;
    let (_, their_collection) = a_cart_with_money_owed(&mut tx, &stranger).await?;

    // The host here says yes to everything, so what refuses is the surface.
    let refused = store::create_payment_session(
        &mut tx,
        &ctx,
        their_collection,
        StartPaymentSession {
            cart_id: my_cart,
            provider_code: "mock".into(),
            context: None,
        },
    )
    .await
    .expect_err("their collection is not mine to pay against");
    assert!(
        refused.is_not_found(),
        "naming somebody else's collection answers not_found, so its amount is not \
         handed back either"
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

/// The other half: the host is now handed the owner, so it can refuse on its
/// own rather than being asked a question with the answer removed.
#[tokio::test]
async fn a_host_is_told_whose_payment_collection_it_is() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let host: Arc<OnlyOwner> = Arc::new(OnlyOwner);
    let mut tx = shop.begin().await;

    payment::register_provider(&mut tx, &shop.ctx(), "mock").await?;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let theirs = a_customer(&mut tx, &shop, "somebody@example.test").await?;

    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        host.as_ref() as &dyn Host,
    );
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: theirs.as_uuid(),
        },
        host.as_ref() as &dyn Host,
    );

    let (my_cart, my_collection) = a_cart_with_money_owed(&mut tx, &ctx).await?;
    let (_, their_collection) = a_cart_with_money_owed(&mut tx, &stranger).await?;

    let refused = store::create_payment_session(
        &mut tx,
        &ctx,
        their_collection,
        StartPaymentSession {
            cart_id: my_cart,
            provider_code: "mock".into(),
            context: None,
        },
    )
    .await
    .expect_err("the host was given an owner and said no");
    assert!(refused.is_denied());

    let opened = store::create_payment_session(
        &mut tx,
        &ctx,
        my_collection,
        StartPaymentSession {
            cart_id: my_cart,
            provider_code: "mock".into(),
            context: None,
        },
    )
    .await?;
    assert_eq!(opened.payment_collection_id, my_collection);

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A collection belongs to the scope it was opened in and to no other.
#[tokio::test]
async fn a_collection_is_not_reachable_from_another_scope() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        shop.host.as_ref() as &dyn Host,
    );
    let (my_cart, my_collection) = a_cart_with_money_owed(&mut tx, &ctx).await?;
    tx.commit().await.expect("to commit");

    let mut elsewhere = shop.begin_as(shop.elsewhere).await;
    let refused = store::create_payment_session(
        &mut elsewhere,
        &shop.theirs(),
        my_collection,
        StartPaymentSession {
            cart_id: my_cart,
            provider_code: "mock".into(),
            context: None,
        },
    )
    .await
    .expect_err("another shop's collection is not there at all");
    assert!(refused.is_not_found());

    drop(elsewhere);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_signed_out_shopper_has_no_account_to_read() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    assert!(
        store::me(&mut tx, &ctx)
            .await
            .expect_err("nobody is signed in")
            .is_denied()
    );
    assert!(
        store::list_my_orders(&mut tx, &ctx, ListPage::default())
            .await
            .expect_err("nor are there orders to list")
            .is_denied()
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_listing_pages_and_stops_at_the_ceiling() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let token = any_token(&mut tx, &ctx).await;
    let over_the_ceiling = MAX_LIMIT + 1;
    for at in 0..over_the_ceiling {
        let made = catalogue::create_product(&mut tx, &ctx, draft(&format!("thing-{at}"))).await?;
        catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    }

    let asked_for_everything = store::list_products(
        &mut tx,
        &ctx,
        &token,
        ListProducts {
            limit: Some(100_000),
            ..ListProducts::default()
        },
    )
    .await?;
    assert_eq!(
        asked_for_everything.len(),
        MAX_LIMIT as usize,
        "a limit beyond the ceiling is brought down to it rather than refused"
    );
    let next = asked_for_everything
        .next
        .clone()
        .expect("a full page offers the next one");

    let rest = store::list_products(
        &mut tx,
        &ctx,
        &token,
        ListProducts {
            after: Some(next),
            limit: Some(100_000),
            ..ListProducts::default()
        },
    )
    .await?;
    assert_eq!(rest.len(), 1, "the page after the ceiling holds the rest");
    assert!(rest.next.is_none());

    let nonsense = store::list_products(
        &mut tx,
        &ctx,
        &token,
        ListProducts {
            after: Some("not-a-cursor".into()),
            ..ListProducts::default()
        },
    )
    .await
    .expect_err("that is not a cursor");
    assert_eq!(nonsense.code(), "invalid");

    drop(tx);
    shop.close().await;
    Ok(())
}

#[test]
fn an_input_refuses_a_field_it_does_not_know() {
    let good: Result<ListProducts, _> = serde_json::from_str(r#"{"limit": 5, "after": "abc"}"#);
    assert!(good.is_ok(), "what the surface documents still parses");

    let sneaky: Result<ListProducts, _> =
        serde_json::from_str(r#"{"limit": 5, "status": "draft"}"#);
    assert!(
        sneaky.is_err(),
        "an unknown field is a mistake or an attempt, and either way not a default"
    );

    let cart: Result<CreateCart, _> =
        serde_json::from_str(r#"{"currency_code": "TRY", "customer_id": "someone else"}"#);
    assert!(
        cart.is_err(),
        "a storefront does not get to say whose cart it is"
    );

    let line: Result<store::AddLineItem, _> = serde_json::from_str(
        r#"{"variant_id": "00000000-0000-0000-0000-000000000000", "quantity": 1,
            "unit_price": "0.01"}"#,
    );
    assert!(line.is_err(), "nor what a thing costs");
}

#[tokio::test]
async fn another_scope_sees_none_of_it() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    let ctx = shop.ctx();
    let made = catalogue::create_product(&mut tx, &ctx, draft("carpet")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    tx.commit().await?;

    let mut elsewhere = shop.begin_as(shop.elsewhere).await;
    let theirs = shop.theirs();
    let their_token = any_token(&mut elsewhere, &theirs).await;

    assert!(
        store::get_product(&mut elsewhere, &theirs, &their_token, "carpet", None)
            .await
            .expect_err("that is another shop's product")
            .is_not_found()
    );
    assert!(
        store::list_products(
            &mut elsewhere,
            &theirs,
            &their_token,
            ListProducts::default()
        )
        .await?
        .is_empty()
    );
    assert!(
        store::get_variant(&mut elsewhere, &theirs, &their_token, uuid_variant())
            .await
            .expect_err("nor is anything else")
            .is_not_found()
    );

    drop(elsewhere);
    shop.close().await;
    Ok(())
}

fn uuid_variant() -> tezgah::id::VariantId {
    tezgah::id::VariantId::from_uuid(Uuid::nil())
}

/// A published product on channel A, with one variant and one option, and a
/// token bound only to channel B — the case #132 was filed over: a B2C key
/// reading the trade catalogue one id at a time because only the listing was
/// filtered.
struct ChannelFixture {
    a_only_product: catalogue::Product,
    variant: catalogue::ProductVariant,
    option: catalogue::ProductOption,
    b_token: String,
}

async fn a_product_gated_to_one_channel(
    tx: &mut Tx<'_>,
    ctx: &tezgah::ports::Ctx<'_>,
) -> tezgah::Result<ChannelFixture> {
    let channel_a = domain_store::create_sales_channel(
        tx,
        ctx,
        NewSalesChannel {
            name: "Trade".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;
    let channel_b = domain_store::create_sales_channel(
        tx,
        ctx,
        NewSalesChannel {
            name: "Retail".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let made = catalogue::create_product(tx, ctx, draft("wholesale-rug")).await?;
    catalogue::publish_product(tx, ctx, made.id).await?;
    catalogue::add_product_to_channel(tx, ctx, made.id, channel_a.id).await?;

    let variant = catalogue::create_variant(
        tx,
        ctx,
        made.id,
        catalogue::NewVariant {
            title: "One size".into(),
            ..NewVariant::default()
        },
    )
    .await?;

    let option = catalogue::add_option(tx, ctx, made.id, "Colour", 0).await?;

    let b_key = domain_store::create_publishable_key(tx, ctx, "b2c").await?;
    domain_store::link_key_to_channel(tx, ctx, b_key.key.id, channel_b.id).await?;

    Ok(ChannelFixture {
        a_only_product: made,
        variant,
        option,
        b_token: b_key.token,
    })
}

#[tokio::test]
async fn a_variant_of_another_channels_product_is_not_read_by_id() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let fixture = a_product_gated_to_one_channel(&mut tx, &ctx).await?;

    let refused = store::get_variant(&mut tx, &ctx, &fixture.b_token, fixture.variant.id)
        .await
        .expect_err("channel B's token was never given this product");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn variants_of_another_channels_product_are_not_listed_by_product_id() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let fixture = a_product_gated_to_one_channel(&mut tx, &ctx).await?;

    let refused = store::list_variants(
        &mut tx,
        &ctx,
        &fixture.b_token,
        ListVariants {
            product_id: fixture.a_only_product.id,
            after: None,
            limit: None,
        },
    )
    .await
    .expect_err("the product itself is not on this token's channel");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_option_of_another_channels_product_is_not_read_by_id() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let fixture = a_product_gated_to_one_channel(&mut tx, &ctx).await?;

    let refused = store::get_product_option(&mut tx, &ctx, &fixture.b_token, fixture.option.id)
        .await
        .expect_err("channel B's token was never given this product's options");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn options_of_another_channels_product_are_not_listed_by_product_id() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let fixture = a_product_gated_to_one_channel(&mut tx, &ctx).await?;

    let refused = store::list_product_options(
        &mut tx,
        &ctx,
        &fixture.b_token,
        ListOptions {
            product_id: fixture.a_only_product.id,
        },
    )
    .await
    .expect_err("the product itself is not on this token's channel");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A product linked to no channel at all is the state every shop starts in
/// (#109's backward-compatibility rule), and stays visible to every token,
/// direct read included.
#[tokio::test]
async fn a_variant_of_an_unchanneled_product_is_read_by_any_token() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("open-to-all")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewVariant {
            title: "One size".into(),
            ..NewVariant::default()
        },
    )
    .await?;

    let token = any_token(&mut tx, &ctx).await;
    let seen = store::get_variant(&mut tx, &ctx, &token, variant.id).await?;
    assert_eq!(seen.id, variant.id);

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A published product on the given channel, with one variant priced at a
/// hundred lira.
async fn a_priced_variant_on_channel(
    tx: &mut Tx<'_>,
    ctx: &tezgah::ports::Ctx<'_>,
    handle: &str,
    channel: tezgah::id::SalesChannelId,
) -> tezgah::Result<tezgah::id::VariantId> {
    let made = catalogue::create_product(tx, ctx, draft(handle)).await?;
    catalogue::publish_product(tx, ctx, made.id).await?;
    catalogue::add_product_to_channel(tx, ctx, made.id, channel).await?;

    let variant = catalogue::create_variant(
        tx,
        ctx,
        made.id,
        NewVariant {
            title: "One size".into(),
            ..NewVariant::default()
        },
    )
    .await?;

    let set = pricing::create_price_set(tx, ctx).await?;
    pricing::link_variant(tx, ctx, variant.id, set.id).await?;
    pricing::add_price(
        tx,
        ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(dec!(100), Currency::parse("TRY")?),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await?;

    Ok(variant.id)
}

/// A cart opened on `channel`, through a publishable key linked to nothing
/// but it — mirroring what a real storefront token looks like.
async fn a_cart_on_channel(
    tx: &mut Tx<'_>,
    ctx: &tezgah::ports::Ctx<'_>,
    channel: tezgah::id::SalesChannelId,
) -> tezgah::Result<CartId> {
    let key = domain_store::create_publishable_key(tx, ctx, "storefront").await?;
    domain_store::link_key_to_channel(tx, ctx, key.key.id, channel).await?;

    let cart = store::create_cart(
        tx,
        ctx,
        &key.token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: Some(channel),
            email: None,
        },
    )
    .await?;
    Ok(cart.id)
}

/// #135: `add_line_item` used to take a variant id and check only that its
/// product was published — never the channel, and it did not even take a
/// token. The cart's own `sales_channel_id` is the authority here (not a
/// token, since the route takes none): a cart opened on one channel cannot
/// receive a line for a variant sold only on another.
#[tokio::test]
async fn a_line_of_another_channels_variant_is_not_added() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel_a = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Trade".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;
    let channel_b = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Retail".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let variant_a =
        a_priced_variant_on_channel(&mut tx, &ctx, "wholesale-rug", channel_a.id).await?;
    let cart_on_b = a_cart_on_channel(&mut tx, &ctx, channel_b.id).await?;

    let refused = store::add_line_item(
        &mut tx,
        &ctx,
        cart_on_b,
        AddLineItem {
            variant_id: variant_a,
            quantity: 1,
            selling_plan_id: None,
        },
    )
    .await
    .expect_err("channel B's cart was never given this channel A product");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

/// The other direction of the same rule: a cart opened on channel A does not
/// take a line for a variant sold only on channel B, even though both
/// channels exist and both are real.
#[tokio::test]
async fn a_cart_on_one_channel_does_not_take_a_line_from_another() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel_a = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Trade".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;
    let channel_b = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Retail".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let variant_b = a_priced_variant_on_channel(&mut tx, &ctx, "retail-lamp", channel_b.id).await?;
    let cart_on_a = a_cart_on_channel(&mut tx, &ctx, channel_a.id).await?;

    let refused = store::add_line_item(
        &mut tx,
        &ctx,
        cart_on_a,
        AddLineItem {
            variant_id: variant_b,
            quantity: 1,
            selling_plan_id: None,
        },
    )
    .await
    .expect_err("channel A's cart was never given this channel B product");
    assert!(refused.is_not_found());

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #109's backward-compatibility rule holds for the write side too: a
/// product linked to no channel at all is addable to a cart on any channel.
#[tokio::test]
async fn a_line_of_an_unchanneled_variant_is_added_from_any_channel() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel_a = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Trade".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let made = catalogue::create_product(&mut tx, &ctx, draft("open-to-all")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        NewVariant {
            title: "One size".into(),
            ..NewVariant::default()
        },
    )
    .await?;
    let set = pricing::create_price_set(&mut tx, &ctx).await?;
    pricing::link_variant(&mut tx, &ctx, variant.id, set.id).await?;
    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(dec!(100), Currency::parse("TRY")?),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await?;

    let cart_on_a = a_cart_on_channel(&mut tx, &ctx, channel_a.id).await?;

    let added = store::add_line_item(
        &mut tx,
        &ctx,
        cart_on_a,
        AddLineItem {
            variant_id: variant.id,
            quantity: 1,
            selling_plan_id: None,
        },
    )
    .await?;
    assert_eq!(added.variant_id, Some(variant.id));

    drop(tx);
    shop.close().await;
    Ok(())
}

async fn a_plan(
    tx: &mut Tx<'_>,
    ctx: &tezgah::ports::Ctx<'_>,
    applies_to: &str,
    variant: tezgah::id::VariantId,
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
            name: "20% off".into(),
            billing_interval_unit: "month".into(),
            billing_interval_count: 1,
            discount_kind: Some("percentage".into()),
            discount_value: Some(dec!(20)),
            applies_to: Some(applies_to.into()),
            ..subscription::NewPlan::default()
        },
    )
    .await?;

    subscription::attach_variant(tx, ctx, plan.id, variant).await?;

    Ok(plan.id)
}

/// #138: a plan's `first_order` discount used to be applied only in the
/// renewal workflow's pricing step, so a shopper subscribing from the
/// storefront was charged the undiscounted price on the order they actually
/// look at.
#[tokio::test]
async fn a_plans_first_order_discount_reaches_the_storefront_cart() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Storefront".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let variant = a_priced_variant_on_channel(&mut tx, &ctx, "coffee-first", channel.id).await?;
    let plan = a_plan(&mut tx, &ctx, "first_order", variant).await?;
    let cart_id = a_cart_on_channel(&mut tx, &ctx, channel.id).await?;

    let added = store::add_line_item(
        &mut tx,
        &ctx,
        cart_id,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    assert_eq!(
        added.unit_price.amount,
        dec!(80),
        "a 20% first-order discount off a hundred lira price"
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

/// The same hole, one shape over: `every_order` never reached checkout either.
#[tokio::test]
async fn a_plans_every_order_discount_reaches_the_storefront_cart() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Storefront".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let variant = a_priced_variant_on_channel(&mut tx, &ctx, "coffee-every", channel.id).await?;
    let plan = a_plan(&mut tx, &ctx, "every_order", variant).await?;
    let cart_id = a_cart_on_channel(&mut tx, &ctx, channel.id).await?;

    let added = store::add_line_item(
        &mut tx,
        &ctx,
        cart_id,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    assert_eq!(added.unit_price.amount, dec!(80));

    drop(tx);
    shop.close().await;
    Ok(())
}

/// A plan's discount that only ever applies to renewals stays off the first
/// order, the one this cart is pricing.
#[tokio::test]
async fn a_plans_renewals_only_discount_stays_off_the_storefront_cart() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    common::a_currency(&mut tx, shop.here, "TRY", 2).await;

    let channel = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Storefront".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    let variant = a_priced_variant_on_channel(&mut tx, &ctx, "coffee-renewals", channel.id).await?;
    let plan = a_plan(&mut tx, &ctx, "renewals", variant).await?;
    let cart_id = a_cart_on_channel(&mut tx, &ctx, channel.id).await?;

    let added = store::add_line_item(
        &mut tx,
        &ctx,
        cart_id,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
            selling_plan_id: Some(plan),
        },
    )
    .await?;

    assert_eq!(added.unit_price.amount, dec!(100));

    drop(tx);
    shop.close().await;
    Ok(())
}

#[test]
fn every_storefront_route_is_under_the_storefront_prefix() {
    let store_routes: Vec<_> = tezgah::api::routes()
        .into_iter()
        .filter(|route| route.surface == tezgah::api::Surface::Store)
        .collect();

    assert!(!store_routes.is_empty());
    for route in &store_routes {
        assert!(
            route.path.starts_with("/store/"),
            "{} is on the storefront surface but not under /store",
            route.path
        );
        assert!(!route.summary.is_empty());
    }
}

/// The other half of the same question: an order is read by whoever it belongs
/// to, and the host is handed that owner rather than nothing.
#[tokio::test]
async fn an_order_is_not_read_by_somebody_else() -> tezgah::Result<()> {
    use rust_decimal_macros::dec;
    use tezgah::money::{Currency, Money};
    use tezgah::order::{self, NewOrder, NewOrderLine};

    let shop = Shop::open().await;
    let host: Arc<OnlyOwner> = Arc::new(OnlyOwner);
    let mut tx = shop.begin().await;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let theirs = a_customer(&mut tx, &shop, "somebody@example.test").await?;

    let lira = Currency::parse("TRY")?;
    let placed = order::create(
        &mut tx,
        &shop.ctx(),
        NewOrder {
            customer_id: Some(mine),
            email: Some("shopper@example.test".into()),
            lines: vec![NewOrderLine::of("A thing", 1, Money::new(dec!(10), lira))],
            ..NewOrder::of(lira)
        },
    )
    .await?;

    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        host.as_ref() as &dyn Host,
    );
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: theirs.as_uuid(),
        },
        host.as_ref() as &dyn Host,
    );

    assert_eq!(
        store::get_my_order(&mut tx, &ctx, placed.id).await?.id,
        placed.id
    );

    let refused = store::get_my_order(&mut tx, &stranger, placed.id)
        .await
        .expect_err("somebody else's order is not theirs to read");
    assert!(refused.is_denied(), "{refused} was not a refusal");

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #183: a cart opened without a region or a channel falls through to the
/// shop's configured default rather than opening with neither.
#[tokio::test]
async fn a_cart_opened_without_a_channel_or_region_gets_the_shops_default() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    common::a_currency(&mut tx, shop.here, "TRY", 2).await;
    a_store_row(&mut tx, &ctx).await;

    let region = domain_store::create_region(
        &mut tx,
        &ctx,
        domain_store::NewRegion {
            name: "default region".into(),
            currency_code: Currency::parse("TRY")?,
            is_tax_inclusive: false,
            has_automatic_taxes: true,
            payment_providers: Vec::new(),
        },
    )
    .await?;

    let channel = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Web".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    domain_store::update_store(
        &mut tx,
        &ctx,
        domain_store::StorePatch {
            default_region_id: Some(region.id),
            default_sales_channel_id: Some(channel.id),
            ..domain_store::StorePatch::default()
        },
    )
    .await?;

    let issued = domain_store::create_publishable_key(&mut tx, &ctx, "storefront").await?;
    domain_store::link_key_to_channel(&mut tx, &ctx, issued.key.id, channel.id).await?;

    let made = store::create_cart(
        &mut tx,
        &ctx,
        &issued.token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;

    assert_eq!(
        made.region_id,
        Some(region.id),
        "the shop's default region, not none"
    );
    let held = cart::get(&mut tx, &ctx, made.id).await?;
    assert_eq!(
        held.sales_channel_id,
        Some(channel.id),
        "the shop's default channel, not none"
    );

    // Naming a region or a channel explicitly still wins over the default.
    let other_channel = domain_store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Marketplace".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;
    domain_store::link_key_to_channel(&mut tx, &ctx, issued.key.id, other_channel.id).await?;

    let chosen = store::create_cart(
        &mut tx,
        &ctx,
        &issued.token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: Some(other_channel.id),
            email: None,
        },
    )
    .await?;
    let held_chosen = cart::get(&mut tx, &ctx, chosen.id).await?;
    assert_eq!(held_chosen.sales_channel_id, Some(other_channel.id));

    // A store's default sales channel cannot be deleted out from under it.
    let refused = domain_store::delete_sales_channel(&mut tx, &ctx, channel.id)
        .await
        .expect_err("the default channel is not deletable while it is the default");
    assert!(refused.is_conflict());

    drop(tx);
    shop.close().await;
    Ok(())
}

/// #186: a region that has restricted its own payment providers only offers
/// those; a region that never has still offers every enabled provider, the
/// same as before this was read at all.
#[tokio::test]
async fn payment_providers_are_narrowed_by_the_carts_region() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    common::a_currency(&mut tx, shop.here, "TRY", 2).await;
    payment::register_provider(&mut tx, &ctx, "stripe").await?;
    payment::register_provider(&mut tx, &ctx, "cod").await?;

    let restricted = domain_store::create_region(
        &mut tx,
        &ctx,
        domain_store::NewRegion {
            name: "restricted".into(),
            currency_code: Currency::parse("TRY")?,
            is_tax_inclusive: false,
            has_automatic_taxes: true,
            payment_providers: vec!["stripe".into()],
        },
    )
    .await?;
    let open = domain_store::create_region(
        &mut tx,
        &ctx,
        domain_store::NewRegion {
            name: "unrestricted".into(),
            currency_code: Currency::parse("TRY")?,
            is_tax_inclusive: false,
            has_automatic_taxes: true,
            payment_providers: Vec::new(),
        },
    )
    .await?;

    let token = any_token(&mut tx, &ctx).await;

    let narrow_cart = store::create_cart(
        &mut tx,
        &ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: Some(restricted.id),
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;
    let narrowed = store::list_payment_providers(
        &mut tx,
        &ctx,
        ListPaymentProviders {
            cart_id: narrow_cart.id,
        },
    )
    .await?;
    let narrowed_codes: Vec<String> = narrowed.into_iter().map(|row| row.code).collect();
    assert_eq!(
        narrowed_codes,
        vec!["stripe".to_string()],
        "cod is not on this region's allow-list"
    );

    let open_cart = store::create_cart(
        &mut tx,
        &ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: Some(open.id),
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;
    let mut everywhere: Vec<String> = store::list_payment_providers(
        &mut tx,
        &ctx,
        ListPaymentProviders {
            cart_id: open_cart.id,
        },
    )
    .await?
    .into_iter()
    .map(|row| row.code)
    .collect();
    everywhere.sort();
    assert_eq!(
        everywhere,
        vec!["cod".to_string(), "stripe".to_string()],
        "a region that never restricted itself offers every enabled provider"
    );

    drop(tx);
    shop.close().await;
    Ok(())
}
