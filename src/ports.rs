//! What tezgah asks of whoever embeds it.
//!
//! Each of these is a decision a host has already made and should not have to
//! make twice: who may do this, what time it is, where an audit row goes, where
//! an event goes. tezgah asks and believes the answer, so a host keeps the
//! authorization engine it already runs instead of being handed a second one.
//!
//! The traits are narrow so an implementor can say what it means; [`Host`] is
//! the one bound everything else takes, and a blanket impl assembles it.
//!
//! # Examples
//!
//! ```
//! use tezgah::ports::{Action, Actor, Authorizer, Permit, Resource};
//!
//! struct LetEveryone;
//!
//! impl Authorizer for LetEveryone {
//!     fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
//!         Ok(Permit::granted())
//!     }
//! }
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

/// The transaction everything runs in. A host opens it, so whatever else that
/// request wrote commits with the order or not at all.
pub type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

/// Whose data this is: one shop, one tenant, one seller. Every table carries
/// it and every row-level security policy reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope(pub Uuid);

impl std::fmt::Display for Scope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Whoever is asking. tezgah does not model roles; it hands this to an
/// [`Authorizer`].
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Actor {
    /// Somebody in the shop's own back office.
    Staff { id: Uuid },
    /// Somebody shopping, signed in.
    Customer { id: Uuid },
    /// Somebody shopping, not signed in, holding a cart.
    Guest { cart: Uuid },
    /// The host: a scheduled job, a provider's webhook, a migration.
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Action {
    View,
    Write,
    Delete,
    /// Moves money: capture, refund, cancel. Always answered separately from
    /// `Write`, because editing an order and refunding one are not one power.
    Settle,
}

/// What is being reached for, carrying the ids an authorizer needs to judge
/// ownership without loading the row first.
///
/// Non-exhaustive: a host matches with a default arm, and a domain added later
/// is denied by that arm rather than failing to compile.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum Resource {
    Product {
        id: Option<Uuid>,
    },
    Cart {
        id: Uuid,
        customer: Option<Uuid>,
    },
    Order {
        id: Uuid,
        /// A third meaning here, past "unowned" and "owned by this
        /// customer": when the id above did not resolve to a row, `None` is
        /// "we do not know whose this is, and will not find out unless you
        /// say yes" — the check that stands between a miss and `not_found`,
        /// so a caller who could never have been told "not yours" is not
        /// told "no such order" either.
        customer: Option<Uuid>,
    },
    Payment {
        id: Uuid,
        order: Uuid,
        /// Whose money is being reached for, when the collection is already
        /// attached to a cart or an order. `None` is "nobody owns it yet",
        /// not "anybody may".
        customer: Option<Uuid>,
    },
    /// A parcel: something that exists because an order is being sent out.
    Fulfillment {
        id: Uuid,
        order: Uuid,
    },
    /// How a shop ships at all — providers, fulfilment sets, service zones,
    /// geo zones, shipping profiles and options. It belongs to no order, and
    /// granting a back office the right to edit it is not granting it the
    /// right to touch a customer's parcel.
    Shipping {
        id: Option<Uuid>,
    },
    Inventory {
        id: Option<Uuid>,
    },
    Customer {
        id: Option<Uuid>,
    },
    Promotion {
        id: Option<Uuid>,
    },
    /// A gift card or a customer's store credit. Its own resource rather than
    /// `Payment`: no provider holds it, and a shop that lets staff refund a
    /// card is not thereby letting them mint balances.
    Credit {
        id: Option<Uuid>,
        /// Whose balance, when it is a named customer's. A gift card is a
        /// bearer instrument and has no owner to name.
        customer: Option<Uuid>,
    },
    /// A recurring contract. Not an [`Resource::Order`]: a shop that lets
    /// somebody cancel a subscription is not thereby letting them edit the
    /// orders it produced, and a host cannot grant the two apart unless they
    /// arrive apart.
    Subscription {
        id: Option<Uuid>,
        customer: Option<Uuid>,
    },
    /// What a customer sees as one order and pays for once — a marketplace's
    /// own row, joining the seller-scoped orders split from it at checkout.
    /// Not an [`Resource::Order`]: a basket lives in the marketplace's scope,
    /// not a seller's, and granting the two apart is the whole point of that
    /// split.
    Basket {
        id: Option<Uuid>,
        customer: Option<Uuid>,
    },
    Pricing,
    /// A seller-scope's payout ledger — who earned what, what the marketplace
    /// took as commission, and what the host has said left the shop. Not
    /// [`Resource::Order`]: reading what an order is worth and reading what a
    /// seller is owed for it are different powers, and a host granting a
    /// support agent the first should not thereby grant the second.
    Payout {
        id: Option<Uuid>,
    },
    /// Shop-wide settings: its name, its default currency, how tax is shown.
    Store,
    /// A currency, a region or a sales channel — what a shop sells in and
    /// through. Not a price.
    Channel {
        id: Option<Uuid>,
    },
    /// A storefront credential. Minting or revoking one is not editing a
    /// price, and a host must be able to grant the two apart.
    PublishableKey {
        id: Option<Uuid>,
    },
    Tax,
    /// A run of the workflow runner, which carries whatever the workflow it
    /// ran was given and returned.
    Workflow {
        id: Option<Uuid>,
        /// The key the run was started with — a cart id, an order id,
        /// whatever the host's own workflow chose — so a host whose runs
        /// belong to somebody can say so. `None` for a query across every
        /// run in scope, which no single owner answers for.
        transaction_key: Option<String>,
    },
}

/// The answer to a question that was asked: an [`Authorizer`] hands one back
/// rather than a `true`, so an answer cannot be ignored by forgetting to read
/// it.
///
/// It is not a key the compiler makes a caller carry. No function in this
/// crate takes a `Permit` as a parameter; every public function that reaches
/// the database calls `ctx.permit(..)` itself or reaches the rows through one
/// that does, and `tests/permit_asked.rs` reads `src/` and fails CI when a new
/// one does neither.
#[derive(Debug, Clone, Copy)]
pub struct Permit(());

impl Permit {
    /// For hosts with no authorization of their own, and for tests.
    pub fn granted() -> Self {
        Permit(())
    }
}

pub trait Authorizer: Send + Sync {
    /// Returns [`Permit`] when allowed. Denial is an error rather than a
    /// `false` so that forgetting to check the answer does not compile.
    fn authorize(&self, actor: &Actor, action: Action, resource: &Resource) -> Result<Permit>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub actor: Actor,
    pub action: Action,
    pub entity: &'static str,
    pub entity_id: Uuid,
    pub summary: serde_json::Value,
}

/// Written in the caller's transaction, so a change that rolls back takes its
/// audit row with it.
#[async_trait]
pub trait AuditSink: Send + Sync {
    async fn record(&self, tx: &mut Tx<'_>, entry: AuditEntry) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct Event {
    /// Dotted and past tense: `order.paid`, `stock.low`.
    pub name: &'static str,
    pub entity_id: Uuid,
    pub payload: serde_json::Value,
}

/// Also written in the caller's transaction: an outbox rather than a publish.
/// Delivering it over the network is the host's, from its own worker.
#[async_trait]
pub trait EventSink: Send + Sync {
    async fn emit(&self, tx: &mut Tx<'_>, event: Event) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct JobSpec {
    pub kind: &'static str,
    pub payload: serde_json::Value,
    pub run_after: Option<DateTime<Utc>>,
}

#[async_trait]
pub trait Jobs: Send + Sync {
    async fn enqueue(&self, tx: &mut Tx<'_>, job: JobSpec) -> Result<()>;
}

/// Everything a host supplies, as one bound. Implement the four narrow traits
/// and this arrives on its own.
pub trait Host: Authorizer + Clock + AuditSink + EventSink + Jobs {}

impl<T> Host for T where T: Authorizer + Clock + AuditSink + EventSink + Jobs {}

/// What a call needs that is not its own arguments. Assembled once per request
/// and passed down; nothing here is read from ambient state.
pub struct Ctx<'a> {
    pub scope: Scope,
    pub actor: Actor,
    host: &'a dyn Host,
}

impl std::fmt::Debug for Ctx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("scope", &self.scope)
            .field("actor", &self.actor)
            .finish_non_exhaustive()
    }
}

impl<'a> Ctx<'a> {
    pub fn new(scope: Scope, actor: Actor, host: &'a dyn Host) -> Self {
        Ctx { scope, actor, host }
    }

    pub fn permit(&self, action: Action, resource: Resource) -> Result<Permit> {
        self.host.authorize(&self.actor, action, &resource)
    }

    pub fn now(&self) -> DateTime<Utc> {
        self.host.now()
    }

    pub async fn audit(&self, tx: &mut Tx<'_>, entry: AuditEntry) -> Result<()> {
        self.host.record(tx, entry).await
    }

    pub async fn emit(&self, tx: &mut Tx<'_>, event: Event) -> Result<()> {
        self.host.emit(tx, event).await
    }

    pub async fn enqueue(&self, tx: &mut Tx<'_>, job: JobSpec) -> Result<()> {
        self.host.enqueue(tx, job).await
    }
}
