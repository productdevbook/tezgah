//! `tezgah-server seed` — the smallest shop a fresh install can check out
//! from: one currency, one region, one sales channel, one stock location, one
//! publishable key. `examples/shop`'s own seeder does the same walk plus a
//! demo product; this one stops short of the product, because a self-hoster
//! reaching for `docker compose up` needs a shop to *point a storefront at*,
//! not a mug it did not ask for — `POST /admin/products` is bound now, and
//! that is how a real catalogue gets in.
//!
//! Idempotent by construction rather than by a check run first: every write
//! here happens in one transaction, and `create_sales_channel` and
//! `create_stock_location` both reject a second row of the same name with
//! `Error::conflict` (`store.rs`, `inventory.rs`). A second run hits that
//! conflict on the sales channel, and the whole transaction — including the
//! region insert ahead of it, which has no such constraint of its own — is
//! dropped rather than committed. Nothing after the conflict runs, so a
//! second `publishable_key` row is never minted either.

use sqlx::PgPool;
use tezgah::id::StockLocationId;
use tezgah::inventory::{self, NewStockLocation};
use tezgah::money::Currency;
use tezgah::page::Paging;
use tezgah::ports::{Actor, Ctx, Host, Scope};
use tezgah::store::{self, NewCurrency, NewRegion, NewSalesChannel};

use crate::host::ServerHost;

const CURRENCY_CODE: &str = "USD";
const CURRENCY_SYMBOL: &str = "$";
const CURRENCY_NAME: &str = "US Dollar";
const REGION_NAME: &str = "Default region";
const CHANNEL_NAME: &str = "Default sales channel";
const LOCATION_NAME: &str = "Default location";
const KEY_TITLE: &str = "Storefront";

pub async fn run(
    pool: &PgPool,
    scope: Scope,
    host: &ServerHost,
    currency_exponent: u32,
) -> tezgah::Result<()> {
    let ctx = Ctx::new(scope, Actor::System, host as &dyn Host);

    let mut tx = pool.begin().await?;
    scope_tx(&mut tx, scope).await?;

    let currency = Currency::parse(CURRENCY_CODE)?;
    store::create_currency(
        &mut tx,
        &ctx,
        NewCurrency {
            code: currency,
            numeric_code: None,
            exponent: currency_exponent as i16,
            symbol: CURRENCY_SYMBOL.to_string(),
            symbol_native: CURRENCY_SYMBOL.to_string(),
            name: CURRENCY_NAME.to_string(),
        },
    )
    .await?;

    store::create_region(
        &mut tx,
        &ctx,
        NewRegion {
            name: REGION_NAME.to_string(),
            currency_code: currency,
            is_tax_inclusive: false,
            has_automatic_taxes: false,
            payment_providers: Vec::new(),
        },
    )
    .await?;

    let channel = match store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: CHANNEL_NAME.to_string(),
            description: None,
            is_disabled: false,
        },
    )
    .await
    {
        Ok(channel) => channel,
        Err(err) if err.code() == "conflict" => {
            drop(tx);
            return report_already_seeded(pool, scope, &ctx).await;
        }
        Err(err) => return Err(err),
    };

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        NewStockLocation {
            name: LOCATION_NAME.to_string(),
            address: None,
        },
    )
    .await?;

    let issued = store::create_publishable_key(&mut tx, &ctx, KEY_TITLE).await?;
    store::link_key_to_channel(&mut tx, &ctx, issued.key.id, channel.id).await?;

    tx.commit().await?;

    println!("seeded a shop");
    println!("  currency        {CURRENCY_CODE}");
    println!("  region          {REGION_NAME}");
    println!("  sales channel   {CHANNEL_NAME}");
    println!("  stock location  {LOCATION_NAME} ({})", location.id);
    println!();
    println!("put these in the environment now — the key is shown once and is not stored:");
    println!("  TEZGAH_STOCK_LOCATION_ID={}", location.id);
    println!("  x-publishable-key: {}", issued.token);

    Ok(())
}

async fn scope_tx(tx: &mut tezgah::ports::Tx<'_>, scope: Scope) -> tezgah::Result<()> {
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// A second run stops at the sales-channel conflict with nothing committed —
/// this looks the shop's already-seeded location back up in a fresh
/// transaction so the operator still gets an id for `TEZGAH_STOCK_LOCATION_ID`
/// even though nothing was written this time.
async fn report_already_seeded(pool: &PgPool, scope: Scope, ctx: &Ctx<'_>) -> tezgah::Result<()> {
    let mut tx = pool.begin().await?;
    scope_tx(&mut tx, scope).await?;
    let page = inventory::stock_locations(&mut tx, ctx, Paging::first(50)).await?;
    tx.commit().await?;

    let existing: Option<StockLocationId> = page
        .items
        .into_iter()
        .find(|location| location.name == LOCATION_NAME)
        .map(|location| location.id);

    println!("already seeded — \"{CHANNEL_NAME}\" already exists, nothing written");
    match existing {
        Some(id) => println!("  TEZGAH_STOCK_LOCATION_ID={id}"),
        None => println!(
            "  no stock location named {LOCATION_NAME:?} was found — list them with GET /admin/stock-locations"
        ),
    }
    println!(
        "the publishable key from the first run is shown once and cannot be read back — issue \
         a new one with POST /admin/publishable-api-keys if it was lost"
    );

    Ok(())
}
