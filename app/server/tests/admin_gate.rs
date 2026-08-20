//! `require_operator` must gate only the admin routes it is actually layered
//! onto — not every path nothing else matched. Before this test existed,
//! `/nope` and `/store/regions` (a path this binary never binds) answered
//! 401 with the admin bearer-token message, because `http::router` used
//! `.layer` rather than `.route_layer`, and `.layer` wraps a router's own
//! fallback along with its routes — see `http::mod`'s `router` for the fix
//! and why.
//!
//! No database is touched: a request that clears the middleware and reaches
//! a handler is not exercised here, and a request that does not is rejected
//! or 404s before any handler runs. `PgPool::connect_lazy` never dials, so
//! this needs nothing running to pass — which is also why the wrong-token
//! case sends something that is not shaped like a session token. A token that
//! is would be looked up, and a lookup needs a database.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tezgah::ports::Scope;
use tezgah_server::host::ServerHost;
use tezgah_server::http::{self, AppState};
use tower::ServiceExt;
use uuid::Uuid;

const ADMIN_TOKEN: &str = "test-only-admin-token";

fn router() -> Router {
    let pool = PgPool::connect_lazy("postgres://example.invalid/tezgah_test_unused")
        .expect("connect_lazy parses the url but never dials it");
    let state = AppState {
        pool,
        host: Arc::new(ServerHost),
        checkout: None,
        scope: Scope(Uuid::nil()),
        admin_token: Some(Arc::from(ADMIN_TOKEN)),
        has_operators: false,
        // Unset, so the callback route is not mounted at all — which is
        // itself worth a case below.
        webhook_secret: None,
    };
    let (router, _bound) = http::router(state);
    router
}

async fn get(router: &Router, path: &str, token: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::builder().method("GET").uri(path);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = request
        .body(Body::empty())
        .expect("a GET with no body always builds");

    let response = router
        .clone()
        .oneshot(request)
        .await
        .expect("an in-process router call never fails to produce a response");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("an axum response body always collects")
        .to_bytes();
    let body = String::from_utf8(bytes.to_vec()).expect("every response body here is UTF-8 JSON");
    (status, body)
}

#[tokio::test]
async fn unbound_path_404s_without_mentioning_admin() {
    let router = router();

    let (status, body) = get(&router, "/nope", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !body.contains("admin"),
        "a path nothing binds must not say the admin surface exists: {body:?}"
    );
}

#[tokio::test]
async fn unbound_store_path_404s_rather_than_asking_for_a_token() {
    // `/store/regions` is not one of `store::router`'s five (or six) bound
    // paths — the admin surface has its own `/admin/regions` — so this must
    // 404 exactly like `/nope`, not challenge for the admin bearer token.
    let router = router();

    let (status, body) = get(&router, "/store/regions", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        !body.contains("admin"),
        "an unbound store path must not say the admin surface exists: {body:?}"
    );
}

#[tokio::test]
async fn bound_admin_path_401s_without_a_token() {
    let router = router();

    let (status, body) = get(&router, "/admin/products", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        body.contains("admin token"),
        "a genuinely bound admin route must still say how to get in: {body:?}"
    );
}

#[tokio::test]
async fn bound_admin_path_401s_with_the_wrong_token() {
    let router = router();

    let (status, _body) = get(&router, "/admin/products", Some("not-the-token")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}
