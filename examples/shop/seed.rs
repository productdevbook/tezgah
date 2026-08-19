//! One shop's worth of data: a currency, a region, a sales channel, a
//! publishable key, one stock location, and one product with one priced and
//! stocked variant — enough for the walkthrough in `main.rs`'s doc comment
//! to run start to finish.
//!
//! Fresh every run. `main` picks a new [`Scope`] each time this is called, so
//! this is a scratch shop rather than a fixture worth keeping between runs —
//! point `DATABASE_URL` at a database you are happy to see filled with one of
//! these on every `cargo run --example shop`.

use rust_decimal_macros::dec;
use sqlx::PgPool;
use tezgah::catalogue::{self, NewProduct, NewVariant, ProductStatus};
use tezgah::id::{StockLocationId, VariantId};
use tezgah::inventory::{self, NewInventoryItem, NewStockLocation};
use tezgah::money::{Currency, Money};
use tezgah::payment;
use tezgah::ports::{Actor, Ctx, Host, JobSpec, Scope};
use tezgah::pricing::{self, NewPrice};
use tezgah::store::{self, NewCurrency, NewRegion, NewSalesChannel, NewStore};
use uuid::Uuid;

use crate::host::ExampleHost;

#[derive(Debug)]
pub struct Seeded {
    pub scope: Scope,
    pub token: String,
    pub location_id: StockLocationId,
    pub product_handle: String,
    pub variant_id: VariantId,
}

pub async fn run(pool: &PgPool, host: &ExampleHost) -> tezgah::Result<Seeded> {
    let scope = Scope(Uuid::now_v7());

    // `tezgah_scope` (`migrations/0001_scope.sql`) is what every row-level
    // security policy checks a scope against; a host serving one shop writes
    // this once and never thinks about it again, per `README.md`.
    sqlx::query("insert into tezgah_scope (id) values ($1)")
        .bind(scope.0)
        .execute(pool)
        .await?;

    let mut tx = pool.begin().await?;
    // `crate::ports::scoped` does exactly this — but it is `pub(crate)`, so
    // a host (this example included) announces its own scope by hand,
    // exactly the way `tests/common/mod.rs::Shop::begin_as` already does.
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await?;

    let ctx = Ctx::new(scope, Actor::System, host as &dyn Host);
    let try_lira = Currency::parse("TRY")?;

    store::create_currency(
        &mut tx,
        &ctx,
        NewCurrency {
            code: try_lira,
            numeric_code: None,
            exponent: 2,
            symbol: "TL".to_string(),
            symbol_native: "TL".to_string(),
            name: "Turkish lira".to_string(),
        },
    )
    .await?;

    let region = store::create_region(
        &mut tx,
        &ctx,
        NewRegion {
            name: "Turkiye".to_string(),
            currency_code: try_lira,
            is_tax_inclusive: false,
            has_automatic_taxes: false,
            payment_providers: Vec::new(),
        },
    )
    .await?;

    let channel = store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Storefront".to_string(),
            description: None,
            is_disabled: false,
        },
    )
    .await?;

    store::create_store(
        &mut tx,
        &ctx,
        NewStore {
            name: "Example shop".to_string(),
            default_currency_code: try_lira,
            supported_currency_codes: vec![try_lira],
            supported_locales: vec!["en".to_string()],
            default_region_id: Some(region.id),
            default_sales_channel_id: Some(channel.id),
            metadata: None,
        },
    )
    .await?;

    let issued = store::create_publishable_key(&mut tx, &ctx, "storefront").await?;
    store::link_key_to_channel(&mut tx, &ctx, issued.key.id, channel.id).await?;

    payment::register_provider(&mut tx, &ctx, "kasapay").await?;

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        NewStockLocation {
            name: "Main warehouse".to_string(),
            address: None,
        },
    )
    .await?;

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        NewInventoryItem {
            sku: Some("mug-001".to_string()),
            title: Some("Mug".to_string()),
            // No `PATCH /store/carts/{id}` is bound in `http.rs` to set a
            // shipping address, so the one product this shop sells is
            // digital-like on purpose — see `main.rs`'s doc comment.
            requires_shipping: false,
        },
    )
    .await?;

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 25, 0).await?;

    let product = catalogue::create_product(
        &mut tx,
        &ctx,
        NewProduct {
            handle: "mug".to_string(),
            title: "A mug".to_string(),
            status: Some(ProductStatus::Published),
            ..NewProduct::default()
        },
    )
    .await?;

    catalogue::add_product_to_channel(&mut tx, &ctx, product.id, channel.id).await?;

    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        product.id,
        NewVariant {
            title: "The only one".to_string(),
            requires_shipping: Some(false),
            ..NewVariant::default()
        },
    )
    .await?;

    inventory::attach_inventory_item(&mut tx, &ctx, variant.id, item.id, 1).await?;

    let set = pricing::create_price_set(&mut tx, &ctx).await?;
    pricing::link_variant(&mut tx, &ctx, variant.id, set.id).await?;
    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(dec!(100), try_lira),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await?;

    // Nothing on the browse → cart → checkout → order path calls
    // `ctx.enqueue` — only a subscription's dunning retry does, in
    // `src/subscription.rs`, and this shop sells nothing on a plan. One job
    // is queued by hand so `host::spawn_worker`'s loop has something real to
    // drain rather than shipping a loop nothing ever exercises.
    ctx.enqueue(
        &mut tx,
        JobSpec {
            kind: "shop.opened",
            payload: serde_json::json!({ "scope": scope.0 }),
            run_after: None,
        },
    )
    .await?;

    tx.commit().await?;

    Ok(Seeded {
        scope,
        token: issued.token,
        location_id: location.id,
        product_handle: product.handle,
        variant_id: variant.id,
    })
}
