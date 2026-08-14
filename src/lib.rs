#![forbid(unsafe_code)]

pub mod error;
pub mod id;
pub mod money;
pub mod page;
pub mod ports;
pub mod store;
pub mod workflow;

pub use error::{Error, Result};
pub use money::{Currency, Money};
pub use page::{Cursor, Page, Paging};
pub use ports::{Action, Actor, Ctx, Permit, Resource, Scope};
pub use workflow::{Failure, Outcome, Step, Workflow};

/// The migrations tezgah owns. A host runs these against its own database,
/// after its own, so the tables land beside what it already has.
pub static MIGRATIONS: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");
