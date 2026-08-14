// Shared by every test binary, so each one leaves some of it unused.
#![allow(dead_code)]

//! A real Postgres, two scopes, and a host that says yes.
//!
//! Every test gets its own database. Not a transaction rolled back at the end:
//! the crate under test opens its own transactions, and half of what is worth
//! testing is what happens when two of them meet.
//!
//! Two scopes are seeded rather than one, because the interesting question is
//! not whether a query returns rows — it is whether it returns somebody else's.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, Host, JobSpec,
    Jobs, Permit, Resource, Scope, Tx,
};
use uuid::Uuid;

/// A host that permits everything and remembers what it was told, so a test
/// can ask whether an audit row was written without a table to read.
#[derive(Debug, Default)]
pub struct Recorder {
    pub audits: parking_lot::Mutex<Vec<(&'static str, Uuid)>>,
    pub events: parking_lot::Mutex<Vec<&'static str>>,
    pub jobs: parking_lot::Mutex<Vec<&'static str>>,
    now: Option<DateTime<Utc>>,
}

impl Recorder {
    pub fn new() -> Arc<Self> {
        Arc::new(Recorder::default())
    }

    /// A clock stopped at a chosen moment, for anything that expires.
    pub fn at(moment: DateTime<Utc>) -> Arc<Self> {
        Arc::new(Recorder {
            now: Some(moment),
            ..Recorder::default()
        })
    }

    pub fn emitted(&self, name: &str) -> bool {
        self.events.lock().contains(&name)
    }

    pub fn queued(&self, kind: &str) -> bool {
        self.jobs.lock().contains(&kind)
    }

    pub fn audited(&self, entity: &str) -> bool {
        self.audits.lock().iter().any(|(seen, _)| *seen == entity)
    }
}

impl Authorizer for Recorder {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Ok(Permit::granted())
    }
}

impl Clock for Recorder {
    fn now(&self) -> DateTime<Utc> {
        self.now.unwrap_or_else(Utc::now)
    }
}

#[async_trait]
impl AuditSink for Recorder {
    async fn record(&self, _: &mut Tx<'_>, entry: AuditEntry) -> tezgah::Result<()> {
        self.audits.lock().push((entry.entity, entry.entity_id));
        Ok(())
    }
}

#[async_trait]
impl EventSink for Recorder {
    async fn emit(&self, _: &mut Tx<'_>, event: Event) -> tezgah::Result<()> {
        self.events.lock().push(event.name);
        Ok(())
    }
}

#[async_trait]
impl Jobs for Recorder {
    async fn enqueue(&self, _: &mut Tx<'_>, job: JobSpec) -> tezgah::Result<()> {
        self.jobs.lock().push(job.kind);
        Ok(())
    }
}

/// A host that refuses everything, for proving a call actually asks.
#[derive(Debug, Default)]
pub struct Doorman;

impl Authorizer for Doorman {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Err(tezgah::Error::denied())
    }
}

impl Clock for Doorman {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for Doorman {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for Doorman {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for Doorman {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

pub struct Shop {
    pub pool: PgPool,
    pub here: Scope,
    /// Somebody else's shop, seeded so isolation can be asserted rather than
    /// assumed.
    pub elsewhere: Scope,
    pub host: Arc<Recorder>,
    name: String,
    admin: PgPool,
}

impl Shop {
    /// A fresh database with the migrations applied and two scopes in it.
    pub async fn open() -> Shop {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".into());

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("a Postgres to test against");

        let name = format!("tezgah_{}", Uuid::now_v7().simple());
        admin
            .execute(format!(r#"create database "{name}""#).as_str())
            .await
            .expect("a database of its own");

        let mut its_url = url::Url::parse(&url).expect("a database url");
        its_url.set_path(&name);

        let pool = PgPoolOptions::new()
            .max_connections(8)
            .connect(its_url.as_str())
            .await
            .expect("its own database");

        tezgah::MIGRATIONS
            .run(&pool)
            .await
            .expect("the migrations to apply");

        let here = Scope(Uuid::now_v7());
        let elsewhere = Scope(Uuid::now_v7());
        for scope in [here, elsewhere] {
            sqlx::query("insert into tezgah_scope (id) values ($1)")
                .bind(scope.0)
                .execute(&pool)
                .await
                .expect("a scope");
        }

        Shop {
            pool,
            here,
            elsewhere,
            host: Recorder::new(),
            name,
            admin,
        }
    }

    pub fn ctx(&self) -> Ctx<'_> {
        Ctx::new(self.here, Actor::System, self.host.as_ref() as &dyn Host)
    }

    /// The same shop seen by somebody else's scope.
    pub fn theirs(&self) -> Ctx<'_> {
        Ctx::new(
            self.elsewhere,
            Actor::System,
            self.host.as_ref() as &dyn Host,
        )
    }

    pub fn ctx_as<'a>(&'a self, actor: Actor, host: &'a dyn Host) -> Ctx<'a> {
        Ctx::new(self.here, actor, host)
    }

    /// A transaction with the scope announced on it, which is the only kind the
    /// row-level security policies admit.
    pub async fn begin(&self) -> Tx<'static> {
        self.begin_as(self.here).await
    }

    pub async fn begin_as(&self, scope: Scope) -> Tx<'static> {
        let mut tx = self.pool.begin().await.expect("a transaction");
        sqlx::query("select set_config('app.scope', $1, true)")
            .bind(scope.0.to_string())
            .execute(&mut *tx)
            .await
            .expect("to announce the scope");
        tx
    }

    /// Drops the database. Called by the test, because a `Drop` cannot await.
    pub async fn close(self) {
        let Shop {
            pool, admin, name, ..
        } = self;
        pool.close().await;
        let _ = admin
            .execute(format!(r#"drop database if exists "{name}" with (force)"#).as_str())
            .await;
    }
}
