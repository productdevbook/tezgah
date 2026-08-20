//! An audit row and an outbox row are written in the caller's transaction —
//! which means a change that rolls back takes both with it.
//!
//! That is the whole claim, and it is invisible in review: `record` reading
//! `&state.pool` instead of the `&mut Tx` the port hands it compiles, passes
//! every test that only checks a row appeared, and leaves an audit trail
//! asserting things that never happened. So this rolls one back and counts.
//!
//! Against a real, disposable Postgres — `DATABASE_URL`, or the same default
//! the workspace's own `tests/common` falls back to.

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use tezgah::ports::{Action, Actor, AuditEntry, AuditSink, Event, EventSink};
use tezgah_server::host::{self, ServerHost};
use uuid::Uuid;

struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
}

impl Database {
    async fn fresh() -> Database {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".into());

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("a Postgres to test against");

        let name = format!("tezgah_server_records_{}", Uuid::now_v7().simple());
        admin
            .execute(format!(r#"create database "{name}""#).as_str())
            .await
            .expect("a database of its own");

        let mut its_url = url::Url::parse(&url).expect("a database url");
        its_url.set_path(&name);

        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(its_url.as_str())
            .await
            .expect("its own database");

        // Only the two tables this binary owns. Nothing here touches a table
        // `tezgah::MIGRATIONS` makes, and the audit trail is not scoped.
        host::create_record_tables(&pool)
            .await
            .expect("the record tables");

        Database { admin, pool, name }
    }

    async fn counts(&self) -> (i64, i64) {
        let audit: i64 = sqlx::query_scalar("select count(*) from server_audit")
            .fetch_one(&self.pool)
            .await
            .expect("server_audit is queryable");
        let events: i64 = sqlx::query_scalar("select count(*) from server_event")
            .fetch_one(&self.pool)
            .await
            .expect("server_event is queryable");
        (audit, events)
    }

    async fn close(self) {
        self.pool.close().await;
        self.admin
            .execute(format!(r#"drop database "{}" with (force)"#, self.name).as_str())
            .await
            .expect("to clean up the database this test made");
    }
}

fn entry(id: Uuid) -> AuditEntry {
    AuditEntry {
        actor: Actor::System,
        action: Action::Write,
        entity: "product",
        entity_id: id,
        summary: serde_json::json!({ "why": "a test" }),
    }
}

fn event(id: Uuid) -> Event {
    Event {
        name: "order.paid",
        entity_id: id,
        payload: serde_json::json!({ "why": "a test" }),
    }
}

#[tokio::test]
async fn a_change_that_rolls_back_leaves_no_record_of_itself() {
    let db = Database::fresh().await;
    let host = ServerHost;
    let id = Uuid::now_v7();

    let mut tx = db.pool.begin().await.expect("a transaction");
    host.record(&mut tx, entry(id)).await.expect("an audit row");
    host.emit(&mut tx, event(id)).await.expect("an outbox row");
    tx.rollback().await.expect("to roll back");

    assert_eq!(
        db.counts().await,
        (0, 0),
        "a rolled-back change left an audit row or an event behind, so one of \
         the sinks is writing outside the transaction it was handed"
    );

    db.close().await;
}

#[tokio::test]
async fn a_change_that_commits_keeps_both() {
    let db = Database::fresh().await;
    let host = ServerHost;
    let id = Uuid::now_v7();

    let mut tx = db.pool.begin().await.expect("a transaction");
    host.record(&mut tx, entry(id)).await.expect("an audit row");
    host.emit(&mut tx, event(id)).await.expect("an outbox row");
    tx.commit().await.expect("to commit");

    assert_eq!(db.counts().await, (1, 1));

    let delivered: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("select delivered_at from server_event")
            .fetch_one(&db.pool)
            .await
            .expect("the event is queryable");
    assert!(
        delivered.is_none(),
        "delivered_at is set, so something in this binary is claiming to have \
         sent an event — nothing here sends one"
    );

    db.close().await;
}

/// `Actor` is `#[non_exhaustive]`, so a kind added to the crate lands in
/// `actor_columns`'s catch-all. An audit log that quietly attributed a new
/// kind of caller to an old one would be worse than one saying it does not
/// know, and this is what says the four it does know are still right.
#[tokio::test]
async fn each_kind_of_caller_is_written_down_as_itself() {
    let db = Database::fresh().await;
    let host = ServerHost;
    let who = Uuid::now_v7();

    for actor in [
        Actor::System,
        Actor::Staff { id: who },
        Actor::Guest { cart: who },
    ] {
        let mut tx = db.pool.begin().await.expect("a transaction");
        host.record(
            &mut tx,
            AuditEntry {
                actor,
                ..entry(Uuid::now_v7())
            },
        )
        .await
        .expect("an audit row");
        tx.commit().await.expect("to commit");
    }

    let kinds: Vec<String> =
        sqlx::query_scalar("select actor_kind from server_audit order by created_at, id")
            .fetch_all(&db.pool)
            .await
            .expect("server_audit is queryable");

    assert_eq!(kinds, vec!["system", "staff", "guest"]);

    db.close().await;
}
