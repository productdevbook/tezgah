//! The library half of `tezgah-server` — `main.rs` is a thin binary over
//! these modules, split out so `tests/` can build a router and drive it
//! with a request in-process, without a live process or a database.

pub mod config;
pub mod deliver;
pub mod host;
pub mod http;
pub mod identity;
pub mod mail;
pub mod provider;
pub mod schedule;
pub mod seed;
pub mod shopper;

use sqlx::PgPool;

/// The tables this binary owns, made in one place.
///
/// `tezgah::MIGRATIONS` builds the commerce schema and knows nothing about a
/// host's own — the jobs it claims, the accounts it authenticates, the
/// shoppers who sign in, and the audit and outbox rows its sinks write. Each
/// of those had its own creator called in its own order from `main`, so a
/// test that wanted a working host had to know the list, and a fifth table
/// would be forgotten at one call site out of several.
///
/// It was: `server_audit` arrived and `seed_idempotent` started failing with
/// `relation "server_audit" does not exist`, because seeding a shop writes an
/// audit row and the test's database had every commerce table and none of
/// these.
pub async fn prepare(pool: &PgPool) -> tezgah::Result<()> {
    host::create_jobs_table(pool).await?;
    host::create_record_tables(pool).await?;
    deliver::prepare(pool).await?;
    identity::create_tables(pool).await?;
    shopper::create_tables(pool).await?;
    Ok(())
}
