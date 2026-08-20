//! `tezgah-server` — a self-hostable HTTP server over the tezgah commerce
//! library, for the same reason `mavi-operator` exists over `mavi`: a
//! library is not something a container starts. `README.md` (this crate's
//! own, in `server/`) carries the route table, what each environment
//! variable does, and the difference between this binary and
//! `examples/shop`.
//!
//! Configuration is read once, at startup, by [`config::Config::from_env`] —
//! a missing `DATABASE_URL` fails here, with one message naming what is
//! wrong, rather than on the first request that needed a pool.

mod config;
mod host;
mod http;
mod provider;
mod seed;

use std::net::SocketAddr;
use std::sync::Arc;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use tezgah::checkout::Checkout;
use tezgah::payment::PaymentProvider;
use tezgah::ports::Scope;
use tokio::net::TcpListener;
use uuid::Uuid;

use config::Config;

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let outcome = match args.next().as_deref() {
        Some("seed") => run_seed().await,
        Some(other) => {
            eprintln!("tezgah-server: unknown subcommand {other:?} — the only one is \"seed\"");
            std::process::exit(2);
        }
        None => run().await,
    };
    if let Err(err) = outcome {
        eprintln!("tezgah-server: {err}");
        std::process::exit(1);
    }
}

/// `tezgah-server seed` — run once, against the same `DATABASE_URL` the
/// server itself uses, to make a fresh install's shop worth pointing a
/// storefront at. `seed::run`'s own doc comment covers why running it twice
/// is safe.
async fn run_seed() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&config.database_url)
        .await?;

    if config.skip_migrations {
        println!("TEZGAH_SKIP_MIGRATIONS=1 — skipping tezgah::MIGRATIONS");
    } else {
        tezgah::MIGRATIONS.run(&pool).await?;
    }

    let scope = bootstrap_scope(&pool).await?;
    let host = host::ServerHost;

    seed::run(&pool, scope, &host, config.currency_exponent).await?;

    Ok(())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;

    let pool = PgPoolOptions::new()
        .max_connections(16)
        .connect(&config.database_url)
        .await?;

    if config.skip_migrations {
        println!("TEZGAH_SKIP_MIGRATIONS=1 — skipping tezgah::MIGRATIONS");
    } else {
        tezgah::MIGRATIONS.run(&pool).await?;
    }

    host::create_jobs_table(&pool).await?;
    host::spawn_worker(pool.clone());

    let scope = bootstrap_scope(&pool).await?;
    let host = Arc::new(host::ServerHost);

    let checkout = match (config.stock_location_id, config.demo_bank_enabled) {
        (Some(location_id), true) => {
            let bank: Arc<dyn kasapay_core::Provider> = Arc::new(provider::DemoBank);
            let kasapay_provider: Arc<dyn PaymentProvider> = Arc::new(
                provider::KasapayProvider::new(bank, config.currency_exponent),
            );
            Some(Arc::new(Checkout::new(kasapay_provider, location_id)))
        }
        (None, _) => {
            println!("checkout not bound: TEZGAH_STOCK_LOCATION_ID is not set");
            None
        }
        (Some(_), false) => {
            println!(
                "checkout not bound: TEZGAH_DEMO_BANK is not set to \
                 i-understand-this-takes-no-money — the only payment provider this binary \
                 ships is a demo that authorises every charge and takes no real money; see \
                 docs/self-hosting.md"
            );
            None
        }
    };

    let admin_token: Option<Arc<str>> = config.admin_token.as_deref().map(Arc::from);
    if admin_token.is_none() {
        println!("admin surface not bound: ADMIN_TOKEN is not set");
    }

    let state = http::AppState {
        pool,
        host,
        checkout,
        scope,
        admin_token,
    };

    let (router, bound) = http::router(state);
    bound.log();

    let port = config.port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    println!("listening on http://{addr}");

    axum::serve(listener, router).await?;

    Ok(())
}

/// One shop, one scope. `README.md`'s "Getting started" says a single-shop
/// host sets `Scope` once and never thinks about it again — this binary
/// creates that one row itself on first boot instead of asking for it as
/// configuration, because `tezgah_scope` (`migrations/0001_scope.sql`) holds
/// at most the one row a single installation ever needs: the first row, or a
/// fresh one if there is none yet.
async fn bootstrap_scope(pool: &PgPool) -> Result<Scope, sqlx::Error> {
    if let Some((id,)) =
        sqlx::query_as::<_, (Uuid,)>("select id from tezgah_scope order by id limit 1")
            .fetch_optional(pool)
            .await?
    {
        return Ok(Scope(id));
    }

    let id = Uuid::now_v7();
    sqlx::query("insert into tezgah_scope (id) values ($1)")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(Scope(id))
}
