//! The five ports tezgah asks of whoever embeds it — see `tezgah::ports` and
//! `docs/hosting.md` in the crate root for what each one is for.
//!
//! `Authorizer` grants every actor, `Actor::System` included: this binary has
//! no roles of its own to check, and `docs/hosting.md` is explicit that
//! denying `Actor::System` silently stops every subscription renewal. What
//! stands between a stranger and the admin surface is `http::admin`'s bearer
//! check, ahead of this port entirely — see that module's doc comment.
//!
//! `AuditSink` and `EventSink` write one JSON line to stdout each, which is
//! enough for a container's own log collection to keep; a host wanting them
//! kept anywhere sturdier replaces this file; nothing else depends on its
//! shape.
//!
//! `Jobs` is the one port that cannot be a stub — `docs/hosting.md` says a
//! no-op implementation has a shop that stops charging somebody and never
//! tries again. `enqueue` writes into `server_job`, a table this binary
//! creates for itself and that tezgah owns no migration for, and
//! `spawn_worker` claims and runs what was written, on its own connection,
//! with `for update skip locked` so two workers cannot take the same row.
//!
//! For a while the second half of that was not true in the way it read: the
//! loop claimed a row, printed it, and marked it processed whatever its kind
//! was. So the one kind tezgah enqueues — a subscription's dunning retry —
//! was swallowed on every tick, and a declined renewal was retried never.
//! [`Dispatcher`] is what a kind is now run by, and a kind nothing handles
//! fails with a reason rather than being marked done.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Arc;

use sqlx::PgPool;
use tezgah::id::SubscriptionId;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, Host, JobSpec,
    Jobs, Permit, Resource, Scope, Tx,
};
use tezgah::subscription::Renewals;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct ServerHost;

impl Authorizer for ServerHost {
    fn authorize(
        &self,
        actor: &Actor,
        action: Action,
        resource: &Resource,
    ) -> tezgah::Result<Permit> {
        match actor {
            // The back office. Which of the five actions a person may take is
            // decided at the door instead, against the `Action` the route
            // table declares — `http::admin`'s `refuse_by_role`. Deciding it
            // twice, in two places that could disagree, is worse than once.
            Actor::Staff { .. } => Ok(Permit::granted()),

            // Scheduled work and provider callbacks. `docs/hosting.md` is
            // explicit that denying this stops every renewal a shop has.
            Actor::System => Ok(Permit::granted()),

            Actor::Customer { id } => shopper_may(*id, action, resource),

            // A guest holds a cart id and nothing else, and this binary reads
            // that id from the same path it is then asked about — so there is
            // nothing here to compare that would refuse anything. The cart id
            // is the credential, which is what a guest cart is.
            Actor::Guest { .. } => Ok(Permit::granted()),
        }
    }
}

/// What somebody signed in to the storefront may do.
///
/// Every kind of resource that has an owner carries it, so this needs no
/// lookup: the question is whether the row belongs to whoever is asking.
///
/// `None` where an owner could have been is refused rather than granted, and
/// that is the interesting half. `Resource::Order`'s own doc says `None` also
/// means "the id did not resolve to a row, and we will not find out unless
/// you say yes" — so granting it would answer `not_found` for an order that
/// does not exist and `denied` for one that does, and the pair tells a
/// stranger which ids are real. Ids here are uuidv7 and carry a timestamp, so
/// that leaks when a shop trades.
fn shopper_may(who: Uuid, action: Action, resource: &Resource) -> tezgah::Result<Permit> {
    let owned_by = |owner: &Option<Uuid>| match owner {
        Some(owner) if *owner == who => Ok(Permit::granted()),
        _ => Err(tezgah::Error::denied()),
    };

    match resource {
        Resource::Cart { customer, .. } => owned_by(customer),
        Resource::Order { customer, .. } => owned_by(customer),
        Resource::Payment { customer, .. } => owned_by(customer),
        Resource::Credit { customer, .. } => owned_by(customer),
        Resource::Subscription { customer, .. } => owned_by(customer),
        Resource::Basket { customer, .. } => owned_by(customer),

        // A customer is the one resource whose id *is* the owner.
        Resource::Customer { id } => match id {
            Some(id) if *id == who => Ok(Permit::granted()),
            _ => Err(tezgah::Error::denied()),
        },

        // The catalogue and what hangs off it: readable by anybody shopping,
        // and writable by nobody who is.
        _ if action == Action::View => Ok(Permit::granted()),
        _ => Err(tezgah::Error::denied()),
    }
}

impl Clock for ServerHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for ServerHost {
    async fn record(&self, _tx: &mut Tx<'_>, entry: AuditEntry) -> tezgah::Result<()> {
        println!(
            "{}",
            serde_json::json!({
                "kind": "audit",
                "action": format!("{:?}", entry.action),
                "entity": entry.entity,
                "entity_id": entry.entity_id,
                "summary": entry.summary,
            })
        );
        Ok(())
    }
}

#[async_trait]
impl EventSink for ServerHost {
    async fn emit(&self, _tx: &mut Tx<'_>, event: Event) -> tezgah::Result<()> {
        println!(
            "{}",
            serde_json::json!({
                "kind": "event",
                "name": event.name,
                "entity_id": event.entity_id,
                "payload": event.payload,
            })
        );
        Ok(())
    }
}

#[async_trait]
impl Jobs for ServerHost {
    async fn enqueue(&self, tx: &mut Tx<'_>, job: JobSpec) -> tezgah::Result<()> {
        sqlx::query(
            "insert into server_job (id, kind, payload, run_after) values ($1, $2, $3, $4)",
        )
        .bind(Uuid::now_v7())
        .bind(job.kind)
        .bind(job.payload)
        .bind(job.run_after)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}

/// Creates the table `enqueue` writes to and the worker reads from.
///
/// Not a tezgah migration — tezgah owns no table by this name and never will,
/// the same way it owns none of a host's own bookkeeping. Made ahead of
/// `tezgah::MIGRATIONS`, carrying no `scope` column and no row-level
/// security, because it is not one of tezgah's tables and nothing in it is a
/// shop's data.
///
/// `alter table ... if not exists` rather than a second create: this table
/// shipped without the three columns a retry needs, and an installation that
/// already has rows in it should keep them.
pub async fn create_jobs_table(pool: &PgPool) -> tezgah::Result<()> {
    sqlx::query(
        "create table if not exists server_job (
             id uuid primary key,
             kind text not null,
             payload jsonb not null,
             run_after timestamptz,
             created_at timestamptz not null default now(),
             processed_at timestamptz
         )",
    )
    .execute(pool)
    .await?;

    for column in [
        "attempts integer not null default 0",
        "failure text",
        "dead_at timestamptz",
    ] {
        sqlx::query(&format!(
            "alter table server_job add column if not exists {column}"
        ))
        .execute(pool)
        .await?;
    }

    Ok(())
}

/// How many times a failing job is tried before it is left alone.
const MAX_ATTEMPTS: i32 = 5;

/// The wait before the next attempt, doubling. Five attempts reach roughly
/// half an hour, which is the right order for a card that was declined and a
/// provider that was briefly unreachable alike.
fn backoff(attempts: i32) -> chrono::Duration {
    chrono::Duration::seconds(60 * i64::from(1 << attempts.clamp(0, 5)))
}

/// What a job kind is dispatched to.
///
/// One kind today, because the crate enqueues one: a subscription's dunning
/// retry. The registry is a `match` rather than a map because a job this
/// binary cannot handle must be visible as such — see [`drain_once`].
pub struct Dispatcher {
    /// `None` today, always: charging a card a shopper left on file needs a
    /// `RecurringProvider`, and kasapay 0.0.5 cannot name the instrument to
    /// take — `provider.rs` carries that in full. A dunning retry fails with
    /// exactly that as its recorded reason rather than being marked done by a
    /// worker that did nothing, which is what used to happen.
    pub renewals: Option<Arc<Renewals>>,
    pub scope: Scope,
}

impl std::fmt::Debug for Dispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dispatcher")
            .field("renewals", &self.renewals.is_some())
            .field("scope", &self.scope)
            .finish()
    }
}

impl Dispatcher {
    async fn run(
        &self,
        pool: &PgPool,
        kind: &str,
        payload: &serde_json::Value,
    ) -> tezgah::Result<()> {
        match kind {
            tezgah::subscription::DUNNING_JOB => {
                let Some(renewals) = self.renewals.as_ref() else {
                    return Err(tezgah::Error::invalid(
                        "this server has no recurring payment provider: no published \
                         kasapay names the saved instrument a stored charge should take, \
                         so nothing here implements RecurringProvider — \
                         productdevbook/kasapay#225 asks for the release that would",
                    ));
                };

                let id = payload
                    .get("subscription")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| Uuid::parse_str(text).ok())
                    .ok_or_else(|| {
                        tezgah::Error::invalid("that job names no subscription to renew")
                    })?;

                let host = ServerHost;
                // `Actor::System`: nobody asked for this. `docs/hosting.md` is
                // explicit that an authorizer denying `Actor::System` stops
                // every renewal a shop has.
                let ctx = Ctx::new(self.scope, Actor::System, &host as &dyn Host);
                renewals
                    .renew(pool, &ctx, SubscriptionId::from_uuid(id))
                    .await?;
                Ok(())
            }
            other => Err(tezgah::Error::invalid(format!(
                "nothing in this binary handles a job of kind {other:?}"
            ))),
        }
    }
}

/// The other half of `Jobs`: a loop that claims what `enqueue` wrote and
/// actually runs it, on a five-second tick.
pub fn spawn_worker(pool: PgPool, dispatcher: Arc<Dispatcher>) {
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            ticks.tick().await;
            if let Err(err) = drain_once(&pool, dispatcher.as_ref()).await {
                eprintln!("job worker: {err}");
            }
        }
    });
}

/// Claims what is due and runs it.
///
/// The claim and the outcome are two transactions on purpose. A job that calls
/// a payment provider holds no row lock while it waits — the claim moves
/// `run_after` out to the next backoff first, so a second worker will not take
/// the same row, and the outcome is written afterwards.
///
/// What changed here, and it is the bug rather than the feature: this loop
/// used to print every row and mark it processed. A kind nothing handled was
/// therefore *done*, silently, and a subscription's dunning retry — the only
/// kind tezgah enqueues — was swallowed on every tick. A job now fails with a
/// reason, waits, and after [`MAX_ATTEMPTS`] is left dead with that reason
/// still on it.
async fn drain_once(pool: &PgPool, dispatcher: &Dispatcher) -> tezgah::Result<()> {
    let due: Vec<(Uuid, String, serde_json::Value, i32)> = {
        let mut tx = pool.begin().await?;

        let claimed: Vec<(Uuid, String, serde_json::Value, i32)> = sqlx::query_as(
            "select id, kind, payload, attempts from server_job
             where processed_at is null
               and dead_at is null
               and (run_after is null or run_after <= now())
             order by created_at
             limit 10
             for update skip locked",
        )
        .fetch_all(&mut *tx)
        .await?;

        for (id, _, _, attempts) in &claimed {
            sqlx::query("update server_job set run_after = $2 where id = $1")
                .bind(id)
                .bind(chrono::Utc::now() + backoff(*attempts))
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        claimed
    };

    for (id, kind, payload, attempts) in due {
        match dispatcher.run(pool, &kind, &payload).await {
            Ok(()) => {
                sqlx::query(
                    "update server_job set processed_at = now(), failure = null where id = $1",
                )
                .bind(id)
                .execute(pool)
                .await?;
                println!(
                    "{}",
                    serde_json::json!({ "kind": "job_ran", "job_kind": kind, "job_id": id })
                );
            }
            Err(err) => {
                let attempts = attempts + 1;
                let dead = attempts >= MAX_ATTEMPTS;
                sqlx::query(
                    "update server_job
                     set attempts = $2,
                         failure = $3,
                         dead_at = case when $4 then now() else null end
                     where id = $1",
                )
                .bind(id)
                .bind(attempts)
                .bind(err.to_string())
                .bind(dead)
                .execute(pool)
                .await?;

                eprintln!(
                    "{}",
                    serde_json::json!({
                        "kind": if dead { "job_dead" } else { "job_failed" },
                        "job_kind": kind,
                        "job_id": id,
                        "attempts": attempts,
                        "failure": err.to_string(),
                    })
                );
            }
        }
    }

    Ok(())
}
