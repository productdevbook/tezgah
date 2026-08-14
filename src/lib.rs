#![forbid(unsafe_code)]

pub mod error;
pub mod id;
pub mod money;
pub mod ports;

pub use error::{Error, Result};
pub use money::{Currency, Money};
pub use ports::{Action, Actor, Ctx, Permit, Resource, Scope};

/// The migrations tezgah owns. A host runs these against its own database,
/// after its own, so the tables land beside what it already has.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
