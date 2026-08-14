//! What tezgah asks of whoever embeds it.
//!
//! Every one of these is a decision a host has already made and should not have
//! to make twice: who is allowed to do this, what time is it, where does an
//! audit row go, where does an event go. tezgah asks rather than answers, so a
//! host that already has an authorization engine keeps using it.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

pub type Tx<'a> = sqlx::Transaction<'a, sqlx::Postgres>;

/// Whose data this is. Every table carries it and every row-level security
/// policy reads it; a host serving one shop still has one, fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Scope(pub Uuid);

/// Whoever is asking. tezgah does not model roles — it hands this to
/// [`Authorizer`] and believes the answer.
#[derive(Debug, Clone)]
pub enum Actor {
    /// A person in the shop's own back office.
    Staff { id: Uuid },
    /// Somebody shopping, signed in.
    Customer { id: Uuid },
    /// Somebody shopping, not signed in, holding a cart.
    Guest { cart: Uuid },
    /// The host itself: a scheduled job, a webhook from a provider, a migration.
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    View,
    Write,
    Delete,
    /// Moves money: capture, refund, cancel. Always a separate answer from Write.
    Settle,
}

/// What is being reached for. Carries the ids an authorizer needs to decide
/// ownership without loading the row itself.
#[derive(Debug, Clone)]
pub enum Resource {
    Product { id: Option<Uuid> },
    Cart { id: Uuid, customer: Option<Uuid> },
    Order { id: Uuid, customer: Option<Uuid> },
    Payment { id: Uuid, order: Uuid },
    Inventory { id: Option<Uuid> },
    Fulfillment { id: Uuid, order: Uuid },
    Pricing,
    Tax,
    Customer { id: Option<Uuid> },
    Promotion { id: Option<Uuid> },
}

/// Proof that a question was asked and answered yes. Repository calls take one,
/// so a code path that never asked cannot reach the data.
#[derive(Debug, Clone, Copy)]
pub struct Permit(());

impl Permit {
    /// For hosts with no authorization of their own, and for tests.
    pub fn granted() -> Self {
        Permit(())
    }
}

pub trait Authorizer: Send + Sync {
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

/// Also written in the caller's transaction — an outbox, not a publish. A host
/// that delivers events over the network does that from its own worker.
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

/// Everything a call needs that is not its own arguments. Assembled once per
/// request by the host and passed down; nothing here is discovered from
/// ambient state.
pub struct Ctx<'a> {
    pub scope: Scope,
    pub actor: Actor,
    pub clock: &'a dyn Clock,
    pub authz: &'a dyn Authorizer,
    pub audit: &'a dyn AuditSink,
    pub events: &'a dyn EventSink,
    pub jobs: &'a dyn Jobs,
}

impl Ctx<'_> {
    pub fn permit(&self, action: Action, resource: Resource) -> Result<Permit> {
        self.authz.authorize(&self.actor, action, &resource)
    }
}
