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

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct ServerHost;

impl Authorizer for ServerHost {
    fn authorize(
        &self,
        _actor: &Actor,
        _action: Action,
        _resource: &Resource,
    ) -> tezgah::Result<Permit> {
        Ok(Permit::granted())
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

/// Creates the table `enqueue` writes to and `spawn_worker` reads from.
///
/// Not a tezgah migration — tezgah owns no table by this name and never will,
/// the same way it owns none of a host's own bookkeeping. Made ahead of
/// `tezgah::MIGRATIONS`, carrying no `scope` column and no row-level
/// security, because it is not one of tezgah's tables and nothing in it is a
/// shop's data.
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
    Ok(())
}

/// The other half of `Jobs`: a loop that claims what `enqueue` wrote and
/// actually runs it, on a five-second tick.
///
/// This binary itself enqueues nothing on the bound routes — only a
/// subscription's dunning retry does, in `src/subscription.rs`, and
/// `/admin/subscriptions` is bound read-only here. The loop still runs,
/// because a job written through some other route this server grows later
/// should not silently wait for a worker that was never started.
pub fn spawn_worker(pool: PgPool) {
    tokio::spawn(async move {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(5));
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
        "select id, kind, payload from server_job
         where processed_at is null and (run_after is null or run_after <= now())
         order by created_at
         limit 10
         for update skip locked",
    )
    .fetch_all(&mut *tx)
    .await?;

    for (id, kind, payload) in due {
        // A real dispatch reads `kind` here — a declined renewal's next
        // attempt, a reminder. Nothing bound yet writes a job this loop needs
        // to act on beyond marking it seen.
        println!(
            "{}",
            serde_json::json!({ "kind": "job_ran", "job_kind": kind, "job_id": id, "payload": payload })
        );
        sqlx::query("update server_job set processed_at = now() where id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}
