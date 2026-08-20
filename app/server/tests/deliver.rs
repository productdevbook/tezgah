//! An event has to leave the building.
//!
//! `EventSink` wrote `server_event` and nothing read it, so `delivered_at`
//! was null on every row for ever. This drives the deliverer against a real
//! receiver — an axum server on a port the kernel picks — and counts what
//! arrived, because "it posts somewhere" is the kind of claim that passes
//! review and fails in production.
//!
//! Against a real, disposable Postgres — `DATABASE_URL`, or the same default
//! the workspace's own `tests/common` falls back to.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use tezgah::ports::{Event, EventSink};
use tezgah_server::deliver::{self, Destination};
use tezgah_server::host::ServerHost;
use uuid::Uuid;

const SECRET: &str = "a-test-secret";

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

        let name = format!("tezgah_server_deliver_{}", Uuid::now_v7().simple());
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

        // Only this binary's own tables. Nothing here touches one
        // `tezgah::MIGRATIONS` makes.
        tezgah_server::host::create_record_tables(&pool)
            .await
            .expect("the record tables");
        deliver::prepare(&pool).await.expect("the retry columns");

        Database { admin, pool, name }
    }

    async fn close(self) {
        self.pool.close().await;
        self.admin
            .execute(format!(r#"drop database "{}" with (force)"#, self.name).as_str())
            .await
            .expect("to clean up the database this test made");
    }
}

#[derive(Clone, Debug)]
struct Receiver {
    seen: Arc<Mutex<Vec<(String, String)>>>,
    answer: StatusCode,
}

/// A real HTTP server on a port the kernel picks. Not a mock: the thing being
/// tested is a request going out over a socket and an answer coming back.
async fn receiver(answer: StatusCode) -> (String, Arc<Mutex<Vec<(String, String)>>>) {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let state = Receiver {
        seen: Arc::clone(&seen),
        answer,
    };

    let app = Router::new()
        .route(
            "/hook",
            post(
                |State(state): State<Receiver>, headers: HeaderMap, body: String| async move {
                    let signature = headers
                        .get("tezgah-signature")
                        .and_then(|value| value.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    state
                        .seen
                        .lock()
                        .expect("nothing panicked while holding this")
                        .push((signature, body));
                    state.answer
                },
            ),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
        .await
        .expect("a port");
    let address = listener.local_addr().expect("its own address");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    (format!("http://{address}/hook"), seen)
}

async fn write_one(pool: &PgPool) -> Uuid {
    let id = Uuid::now_v7();
    let mut tx = pool.begin().await.expect("a transaction");
    ServerHost
        .emit(
            &mut tx,
            Event {
                name: "order.paid",
                entity_id: id,
                payload: serde_json::json!({ "total": "12.00" }),
            },
        )
        .await
        .expect("an outbox row");
    tx.commit().await.expect("to commit");
    id
}

async fn drain(pool: &PgPool, url: &str) {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("an http client");
    deliver::drain_once(
        pool,
        &client,
        &Destination {
            url: Arc::from(url),
            secret: Arc::from(SECRET),
        },
    )
    .await
    .expect("a drain that does not error");
}

#[tokio::test]
async fn an_event_is_sent_once_signed_and_marked_delivered() {
    let db = Database::fresh().await;
    let (url, seen) = receiver(StatusCode::OK).await;

    let entity = write_one(&db.pool).await;
    drain(&db.pool, &url).await;

    let (signature, body) = {
        let arrived = seen.lock().expect("nothing panicked while holding this");
        assert_eq!(
            arrived.len(),
            1,
            "the receiver saw {} events",
            arrived.len()
        );
        arrived[0].clone()
    };

    assert_eq!(
        signature,
        deliver::signature(SECRET, &body),
        "the signature does not match the exact bytes that were sent"
    );

    let sent: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(sent["name"], "order.paid");
    assert_eq!(sent["entity_id"], entity.to_string());
    assert_eq!(sent["payload"]["total"], "12.00");

    let (delivered, failure): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("select delivered_at, failure from server_event")
            .fetch_one(&db.pool)
            .await
            .expect("the row is queryable");
    assert!(delivered.is_some(), "delivered_at is still null");
    assert!(failure.is_none());

    // A second tick must not send it again: the claim looks at delivered_at.
    drain(&db.pool, &url).await;
    assert_eq!(
        seen.lock()
            .expect("nothing panicked while holding this")
            .len(),
        1,
        "a delivered event was sent a second time"
    );

    db.close().await;
}

#[tokio::test]
async fn a_refused_event_waits_and_keeps_its_reason() {
    let db = Database::fresh().await;
    let (url, seen) = receiver(StatusCode::INTERNAL_SERVER_ERROR).await;

    write_one(&db.pool).await;
    drain(&db.pool, &url).await;

    assert_eq!(
        seen.lock()
            .expect("nothing panicked while holding this")
            .len(),
        1
    );

    type Row = (
        i32,
        Option<String>,
        Option<chrono::DateTime<chrono::Utc>>,
        Option<chrono::DateTime<chrono::Utc>>,
    );
    let (attempts, failure, delivered, next): Row =
        sqlx::query_as("select attempts, failure, delivered_at, next_attempt_at from server_event")
            .fetch_one(&db.pool)
            .await
            .expect("the row is queryable");

    assert_eq!(attempts, 1);
    assert_eq!(failure.as_deref(), Some("answered 500"));
    assert!(delivered.is_none(), "a refused event was marked delivered");
    assert!(
        next.is_some_and(|at| at > chrono::Utc::now()),
        "a refused event is due again immediately, so the next tick spins on it"
    );

    // Due in the future, so a second tick takes nothing.
    drain(&db.pool, &url).await;
    assert_eq!(
        seen.lock()
            .expect("nothing panicked while holding this")
            .len(),
        1,
        "a waiting event was sent again before its backoff elapsed"
    );

    db.close().await;
}

#[tokio::test]
async fn an_event_that_keeps_failing_is_left_dead_with_its_reason() {
    let db = Database::fresh().await;
    let (url, _seen) = receiver(StatusCode::BAD_GATEWAY).await;

    write_one(&db.pool).await;

    // One attempt short of the limit, and due now.
    sqlx::query("update server_event set attempts = 4, next_attempt_at = null")
        .execute(&db.pool)
        .await
        .expect("to age the row");

    drain(&db.pool, &url).await;

    let (dead, failure): (Option<chrono::DateTime<chrono::Utc>>, Option<String>) =
        sqlx::query_as("select dead_at, failure from server_event")
            .fetch_one(&db.pool)
            .await
            .expect("the row is queryable");

    assert!(dead.is_some(), "a spent event is still being retried");
    assert_eq!(failure.as_deref(), Some("answered 502"));

    db.close().await;
}

/// A fixed vector, so the signature cannot change shape without somebody
/// saying so. Every receiver already written against it would stop verifying.
#[test]
fn the_signature_is_hmac_sha256_of_the_body() {
    assert_eq!(
        deliver::signature("a-test-secret", r#"{"hello":"world"}"#),
        "sha256=b50822091328e03b8b70cd54ea83551e5d91f5cdff8e64aa54ad6eca52c9773a"
    );
}
