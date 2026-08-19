//! The five ports, at their smallest honest size.
//!
//! `Authorizer` grants everything — an example shop has no roles of its own —
//! but it answers `Actor::System` the same as every other actor rather than
//! carving out an exception: `docs/hosting.md` is explicit that denying it
//! silently stops every subscription renewal, and nothing here should read as
//! a safe pattern to copy that mistake from.
//!
//! `AuditSink` and `EventSink` write one line to stdout each. `Jobs` is the
//! one port that cannot be a stub: `enqueue` writes into its own `shop_job`
//! table inside the caller's transaction, and `spawn_worker` is a real loop,
//! on its own connection, that claims due rows with `for update skip locked`
//! and runs them — see the module doc on `main.rs` for why this walkthrough
//! still queues one job by hand to give that loop something to drain.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct ExampleHost;

impl Authorizer for ExampleHost {
    fn authorize(
        &self,
        _actor: &Actor,
        _action: Action,
        _resource: &Resource,
    ) -> tezgah::Result<Permit> {
        // A real host checks its own roles here. This one has none, and
        // grants every actor alike — Actor::System included.
        Ok(Permit::granted())
    }
}

impl Clock for ExampleHost {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for ExampleHost {
    async fn record(&self, _tx: &mut Tx<'_>, entry: AuditEntry) -> tezgah::Result<()> {
        println!(
            "audit: {:?} {} {} {}",
            entry.action, entry.entity, entry.entity_id, entry.summary
        );
        Ok(())
    }
}

#[async_trait]
impl EventSink for ExampleHost {
    async fn emit(&self, _tx: &mut Tx<'_>, event: Event) -> tezgah::Result<()> {
        println!(
            "event: {} {} {}",
            event.name, event.entity_id, event.payload
        );
        Ok(())
    }
}

#[async_trait]
impl Jobs for ExampleHost {
    async fn enqueue(&self, tx: &mut Tx<'_>, job: JobSpec) -> tezgah::Result<()> {
        sqlx::query("insert into shop_job (id, kind, payload, run_after) values ($1, $2, $3, $4)")
            .bind(Uuid::now_v7())
            .bind(job.kind)
            .bind(job.payload)
            .bind(job.run_after)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

/// Creates the table `enqueue` writes to and `spawn_worker` reads from.
///
/// Not a tezgah migration — tezgah owns no table by this name and never
/// will, the same way it owns none of a host's other bookkeeping. This is
/// made the way a host makes any table of its own, ahead of
/// `tezgah::MIGRATIONS`, and carries no `scope` column or row-level security
/// because it is not one of tezgah's tables and nothing in it is a shop's
/// data.
pub async fn create_jobs_table(pool: &PgPool) -> tezgah::Result<()> {
    sqlx::query(
        "create table if not exists shop_job (
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
    Ok(())
}

/// The other half of `Jobs`: a loop that claims what `enqueue` wrote and
/// actually runs it, on a two-second tick. `docs/hosting.md` is explicit
/// that a host implementing `Jobs` as a no-op has a shop that stops charging
/// somebody and never tries again — this is the alternative, at whatever
/// small scale an example can show it.
pub fn spawn_worker(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(2));
        loop {
            ticks.tick().await;
            if let Err(err) = drain_once(&pool).await {
                eprintln!("job worker: {err}");
            }
        }
    });
}

async fn drain_once(pool: &PgPool) -> tezgah::Result<()> {
    let mut tx = pool.begin().await?;

    let due: Vec<(Uuid, String, serde_json::Value)> = sqlx::query_as(
        "select id, kind, payload from shop_job
         where processed_at is null and (run_after is null or run_after <= now())
         order by created_at
         limit 10
         for update skip locked",
    )
    .fetch_all(&mut *tx)
    .await?;

    for (id, kind, payload) in due {
        // A real host dispatches on `kind` here — a declined renewal's next
        // attempt, a reminder. This example has exactly one kind, queued
        // once by `seed::run`, and running it is printing that it ran.
        println!("job: ran {kind} {id} {payload}");
        sqlx::query("update shop_job set processed_at = now() where id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}
