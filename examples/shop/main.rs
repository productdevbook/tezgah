//! The smallest way to embed tezgah: enough of its five host ports and a
//! `PaymentProvider` over kasapay-core to walk catalogue → cart → checkout →
//! order as plain library calls, against a real Postgres — issue #199, filed
//! because nothing in this repository stood one up.
//!
//! # This is not a server
//!
//! [`app/server/`](../../app/server) is that: a real binary, reading `PORT` and
//! `DATABASE_URL` from its environment, serving HTTP for as long as it runs.
//! This file calls `tezgah::api::store` functions directly, the same
//! functions `app/server/src/http/store.rs`'s handlers call, minus the framework
//! that would turn them into routes — there is no listener here and nothing
//! to `curl`. Read this file to see what embedding tezgah looks like at its
//! smallest; read `app/server/` to see it made into something you would deploy.
//! `axum` is not a dependency of this crate at all for exactly that reason:
//! nobody consuming tezgah as a library is handed an HTTP framework they did
//! not ask for, and this example does not need one to make its point.
//!
//! # Running it
//!
//! A Postgres 15+ this example may create tables and a scope in freely — it
//! is not meant to survive between runs:
//!
//! ```text
//! createdb tezgah_shop
//! DATABASE_URL=postgres://postgres:postgres@localhost:5432/tezgah_shop \
//!     cargo run --example shop
//! ```
//!
//! `DATABASE_URL` defaults to `postgres://postgres:postgres@localhost:5432/postgres`
//! — the same default `tests/common` uses.
//!
//! # What it does
//!
//! Runs `tezgah::MIGRATIONS`, then seeds one shop (`seed.rs`): a currency, a
//! region, a sales channel, a publishable key, one stock location, and one
//! product with one priced and stocked variant. Then it calls, in order:
//!
//! - `store::list_products` and `store::get_product` — the catalogue
//! - `store::create_cart` and `store::add_line_item` — opening a cart and
//!   adding the seeded variant to it
//! - `store::complete_cart` — checkout, through `checkout::Checkout` and the
//!   `KasapayProvider` in `provider.rs`
//!
//! Each call opens its own transaction and commits it, the same boundary an
//! HTTP handler would draw around one request — see `begin` below.
//!
//! # What it does not do
//!
//! `tests/snapshots/openapi.json` names 486 operations; this walks five of
//! them by calling their handler functions directly. Every admin route,
//! every catalogue write, and every agreement, credit, digital,
//! inventory-lot, order-basket, payout, subscription and tax-identity route
//! is untouched — this is a storefront walkthrough, not a panel, and
//! `tests/reachable.rs`, in the crate proper, is what keeps every domain
//! function honest about having *a* route regardless of whether this example
//! calls it.
//!
//! The product this shop seeds is `requires_shipping: false`, so the
//! walkthrough never needs to set a shipping address.

mod host;
mod provider;
mod seed;

use std::sync::Arc;

use sqlx::PgPool;
use tezgah::api::store;
use tezgah::checkout::Checkout;
use tezgah::payment::PaymentProvider;
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};

/// The shop's one currency's decimal places — `seed.rs` enables `TRY` at
/// this exponent, and `provider::KasapayProvider` is fixed to it for the
/// reason its own doc comment gives.
const CURRENCY_EXPONENT: u32 = 2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await?;

    tezgah::MIGRATIONS.run(&pool).await?;
    host::create_jobs_table(&pool).await?;
    host::spawn_worker(pool.clone());

    let host = Arc::new(host::ExampleHost);
    let seeded = seed::run(&pool, &host).await?;
    println!("product handle: {}", seeded.product_handle);
    println!("variant id:     {}", seeded.variant_id);

    let bank: Arc<dyn kasapay_core::Provider> = Arc::new(provider::DemoBank);
    let kasapay_provider: Arc<dyn PaymentProvider> =
        Arc::new(provider::KasapayProvider::new(bank, CURRENCY_EXPONENT));
    let checkout = Checkout::new(kasapay_provider, seeded.location_id);

    let guest = Actor::Guest {
        cart: uuid::Uuid::nil(),
    };

    let mut tx = begin(&pool, seeded.scope).await?;
    let ctx = Ctx::new(seeded.scope, guest.clone(), host.as_ref() as &dyn Host);
    let catalogue =
        store::list_products(&mut tx, &ctx, &seeded.token, store::ListProducts::default()).await?;
    tx.commit().await?;
    println!("catalogue: {} product(s)", catalogue.items.len());

    let mut tx = begin(&pool, seeded.scope).await?;
    let ctx = Ctx::new(seeded.scope, guest.clone(), host.as_ref() as &dyn Host);
    let product =
        store::get_product(&mut tx, &ctx, &seeded.token, &seeded.product_handle, None).await?;
    tx.commit().await?;
    println!("fetched product: {}", product.title);

    let mut tx = begin(&pool, seeded.scope).await?;
    let ctx = Ctx::new(seeded.scope, guest.clone(), host.as_ref() as &dyn Host);
    let cart = store::create_cart(
        &mut tx,
        &ctx,
        &seeded.token,
        store::CreateCart {
            currency_code: "TRY".to_string(),
            region_id: None,
            sales_channel_id: None,
            email: Some("shopper@example.com".to_string()),
        },
    )
    .await?;
    tx.commit().await?;
    println!("opened cart: {}", cart.id);

    let cart_actor = Actor::Guest {
        cart: cart.id.as_uuid(),
    };

    let mut tx = begin(&pool, seeded.scope).await?;
    let ctx = Ctx::new(seeded.scope, cart_actor.clone(), host.as_ref() as &dyn Host);
    let line = store::add_line_item(
        &mut tx,
        &ctx,
        cart.id,
        store::AddLineItem {
            variant_id: seeded.variant_id,
            quantity: 1,
            selling_plan_id: None,
        },
    )
    .await?;
    tx.commit().await?;
    println!(
        "added line item: {} x {}",
        line.quantity, line.product_title
    );

    let mut tx = begin(&pool, seeded.scope).await?;
    let ctx = Ctx::new(seeded.scope, cart_actor, host.as_ref() as &dyn Host);
    let completed = store::complete_cart(&mut tx, &ctx, cart.id, &checkout, &pool).await?;
    tx.commit().await?;

    match completed.order_id {
        Some(order_id) => println!("placed order: {order_id}"),
        None => println!(
            "checkout did not finish: requires_more = {}",
            completed.requires_more
        ),
    }

    Ok(())
}

/// Opens a transaction and announces this shop's scope on it —
/// `crate::ports::scoped` does exactly this inside the crate, but it is
/// `pub(crate)`, so, like `seed.rs`, this writes the two lines by hand.
async fn begin(pool: &PgPool, scope: Scope) -> tezgah::Result<Tx<'static>> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
