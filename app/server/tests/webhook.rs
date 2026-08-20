//! A payment provider's callback: refused unless signed, recorded once.
//!
//! `payment::record_webhook` was written, tested and reachable from nothing —
//! no route in `src/api/` declared a path a provider could post to, so any
//! payment confirmed asynchronously (3-D Secure, a hosted form, a bank
//! transfer) had nowhere to be confirmed *to*. This drives the route that now
//! receives one.
//!
//! Two of the three cases need no database: a refused signature is refused
//! before any handler runs. The third does, because recording a delivery is
//! a row.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use tezgah::ports::Scope;
use tezgah_server::deliver::signature;
use tezgah_server::host::ServerHost;
use tezgah_server::http::{self, AppState};
use tower::ServiceExt;
use uuid::Uuid;

const SECRET: &str = "a-test-webhook-secret";

fn router_with(pool: PgPool, scope: Scope, secret: Option<&str>) -> Router {
    let state = AppState {
        pool,
        host: Arc::new(ServerHost),
        checkout: None,
        scope,
        admin_token: Some(Arc::from("test-only-admin-token")),
        has_operators: false,
        webhook_secret: secret.map(Arc::from),
        mailer: None,
        panel_url: None,
        files: None,
    };
    let (router, _bound) = http::router(state);
    router
}

/// No database is dialled: every case here is refused or 404s before a
/// handler runs.
fn offline(secret: Option<&str>) -> Router {
    let pool = PgPool::connect_lazy("postgres://example.invalid/tezgah_test_unused")
        .expect("connect_lazy parses the url but never dials it");
    router_with(pool, Scope(Uuid::nil()), secret)
}

async fn post(router: &Router, body: &str, sent: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method("POST")
        .uri("/webhooks/payments/demo-bank")
        .header("content-type", "application/json");
    if let Some(signature) = sent {
        request = request.header("x-provider-signature", signature);
    }
    let request = request
        .body(Body::from(body.to_owned()))
        .expect("a POST always builds");

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("a body")
        .to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

fn callback(event_id: &str) -> String {
    serde_json::json!({
        "event_id": event_id,
        "event_type": "payment_intent.succeeded",
        "kind": "authorized",
        "session_id": null,
        "amount": null,
        "payload": { "whatever": "the provider sent" },
    })
    .to_string()
}

#[tokio::test]
async fn without_a_secret_the_route_does_not_exist() {
    let router = offline(None);
    let body = callback("evt_1");
    let (status, _) = post(&router, &body, Some(&signature(SECRET, &body))).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the callback route is mounted with no secret to check against"
    );
}

#[tokio::test]
async fn a_wrong_or_missing_signature_is_refused() {
    let router = offline(Some(SECRET));
    let body = callback("evt_1");

    for sent in [
        None,
        Some(""),
        Some("sha256=not-a-signature"),
        // Right shape, wrong key.
        Some(signature("another-shop's-secret", &body).as_str()),
        // Right key, different body — which is what a replay with an edited
        // amount looks like.
        Some(signature(SECRET, &callback("evt_2")).as_str()),
    ] {
        let (status, _) = post(&router, &body, sent).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "a callback signed {sent:?} was not refused"
        );
    }
}

struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
    scope: Scope,
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

        let name = format!("tezgah_server_webhook_{}", Uuid::now_v7().simple());
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

        tezgah::MIGRATIONS
            .run(&pool)
            .await
            .expect("the migrations to apply");
        tezgah_server::prepare(&pool)
            .await
            .expect("the tables this binary owns");

        let scope = Scope(Uuid::now_v7());
        sqlx::query("insert into tezgah_scope (id) values ($1)")
            .bind(scope.0)
            .execute(&pool)
            .await
            .expect("a scope to receive into");

        // In a transaction that announces the scope, because the policies on
        // every registered table admit no other kind — the same `set_config`
        // `http::begin` does for a request.
        let mut tx = pool.begin().await.expect("a transaction");
        sqlx::query("select set_config('app.scope', $1, true)")
            .bind(scope.0.to_string())
            .execute(&mut *tx)
            .await
            .expect("to announce the scope");
        sqlx::query(
            "insert into payment_provider (id, scope, code, is_enabled)
             values ($1, $2, 'demo-bank', true)",
        )
        .bind(Uuid::now_v7())
        .bind(scope.0)
        .execute(&mut *tx)
        .await
        .expect("a provider to receive for");
        tx.commit().await.expect("to commit");

        Database {
            admin,
            pool,
            name,
            scope,
        }
    }

    async fn close(self) {
        self.pool.close().await;
        self.admin
            .execute(format!(r#"drop database "{}" with (force)"#, self.name).as_str())
            .await
            .expect("to clean up the database this test made");
    }
}

/// The point of writing a callback down at all: a provider that sends the
/// same delivery twice must change nothing the second time.
#[tokio::test]
async fn a_redelivery_lands_once() {
    let db = Database::fresh().await;
    let router = router_with(db.pool.clone(), db.scope, Some(SECRET));
    let body = callback("evt_the_same_one");
    let signed = signature(SECRET, &body);

    let (status, first) = post(&router, &body, Some(&signed)).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let first: serde_json::Value = serde_json::from_str(&first).expect("json");
    assert_eq!(first["recorded"], true);

    let (status, again) = post(&router, &body, Some(&signed)).await;
    assert_eq!(status, StatusCode::OK, "{again}");
    let again: serde_json::Value = serde_json::from_str(&again).expect("json");
    assert_eq!(
        again["recorded"], false,
        "the second delivery was recorded as a new one"
    );

    let mut tx = db.pool.begin().await.expect("a transaction");
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(db.scope.0.to_string())
        .execute(&mut *tx)
        .await
        .expect("to announce the scope");
    let rows: i64 = sqlx::query_scalar("select count(*) from payment_webhook_event")
        .fetch_one(&mut *tx)
        .await
        .expect("the table is queryable");
    assert_eq!(rows, 1, "a redelivery wrote a second row");

    db.close().await;
}
