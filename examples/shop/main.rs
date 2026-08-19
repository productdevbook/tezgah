//! A minimal running shop: enough of tezgah's HTTP surface, its five host
//! ports, and a `PaymentProvider` over kasapay-core to walk one browser from
//! the catalogue to a placed order — issue #199, filed because nothing in
//! this repository stood one up.
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
//! — the same default `tests/common` uses. `PORT` defaults to `3000`.
//!
//! # What it does
//!
//! Runs `tezgah::MIGRATIONS`, then seeds one shop (`seed.rs`): a currency, a
//! region, a sales channel, a publishable key, one stock location, and one
//! product with one priced and stocked variant. Then it serves six routes:
//!
//! | Method | Path | What |
//! |---|---|---|
//! | GET | `/store/products` | the catalogue |
//! | GET | `/store/products/{handle}` | one product |
//! | POST | `/store/carts` | open a cart |
//! | GET | `/store/carts/{id}` | read it back |
//! | POST | `/store/carts/{id}/line-items` | add to it |
//! | POST | `/store/carts/{id}/complete` | check out |
//!
//! The two `/store/products` routes and `POST /store/carts` read the
//! seeded publishable key as `x-publishable-key` — see `http.rs`'s doc
//! comment for why the cart-by-id routes below need no such header. Both
//! the token and the variant id this shop sells are printed to stdout at
//! startup. Walk the whole flow with them:
//!
//! ```text
//! curl localhost:3000/store/products -H 'x-publishable-key: <token>'
//!
//! curl -X POST localhost:3000/store/carts \
//!     -H 'x-publishable-key: <token>' -H 'content-type: application/json' \
//!     -d '{"currency_code":"TRY","email":"shopper@example.com"}'
//!
//! curl -X POST localhost:3000/store/carts/<cart id>/line-items \
//!     -H 'content-type: application/json' \
//!     -d '{"variant_id":"<variant id>","quantity":1}'
//!
//! curl -X POST localhost:3000/store/carts/<cart id>/complete
//! ```
//!
//! The last call answers an order id: catalogue → cart → checkout → order,
//! all the way through, against a real router and a real Postgres.
//!
//! # What it does not do
//!
//! `tests/snapshots/openapi.json` names 483 operations; `http.rs` binds six
//! of them, by hand. Every admin route, every catalogue write, and every
//! agreement, credit, digital, inventory-lot, order-basket, payout,
//! subscription and tax-identity route stays unbound — this is a storefront
//! walkthrough, not a panel. Nothing here generates or checks the binding
//! either: `tests/reachable.rs`, in the crate proper, is what keeps every
//! domain function honest about having *a* route; this example is one
//! host's choice of which of those routes it will actually serve, and nine
//! times out of ten more that choice does not reach.
//!
//! The product this shop seeds is `requires_shipping: false`, so the
//! walkthrough never needs `PATCH /store/carts/{id}` to set an address —
//! one honestly unavoidable route among the six earns its place over an
//! address form.
//!
//! `axum` is a `dev-dependency` here and nowhere else: nothing under `src/`
//! depends on it, and nobody consuming tezgah as a library is handed an HTTP
//! framework they did not ask for.

mod host;
mod http;
mod provider;
mod seed;

use std::net::SocketAddr;
use std::sync::Arc;

use tezgah::checkout::Checkout;
use tezgah::payment::PaymentProvider;
use tokio::net::TcpListener;

/// The shop's one currency's decimal places — `seed.rs` enables `TRY` at
/// this exponent, and `provider::KasapayProvider` is fixed to it for the
/// reason its own doc comment gives.
const CURRENCY_EXPONENT: u32 = 2;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3000);

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(8)
        .connect(&database_url)
        .await?;

    tezgah::MIGRATIONS.run(&pool).await?;
    host::create_jobs_table(&pool).await?;
    host::spawn_worker(pool.clone());

    let host = Arc::new(host::ExampleHost);
    let seeded = seed::run(&pool, &host).await?;
    println!("publishable key: {}", seeded.token);
    println!("product handle:  {}", seeded.product_handle);
    println!("variant id:      {}", seeded.variant_id);

    let bank: Arc<dyn kasapay_core::Provider> = Arc::new(provider::DemoBank);
    let kasapay_provider: Arc<dyn PaymentProvider> =
        Arc::new(provider::KasapayProvider::new(bank, CURRENCY_EXPONENT));
    let checkout = Arc::new(Checkout::new(kasapay_provider, seeded.location_id));

    let state = http::AppState {
        pool,
        host,
        checkout,
        scope: seeded.scope,
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    println!("listening on http://{addr}");

    axum::serve(listener, http::router(state)).await?;

    Ok(())
}
