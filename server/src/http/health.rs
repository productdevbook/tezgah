//! `GET /health` — not "the process is running", which a probe can already
//! tell from the socket accepting a connection, but "a query against
//! Postgres still answers". A pool that has quietly lost its database is the
//! failure a liveness probe exists to catch.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use super::AppState;

pub async fn check(State(state): State<AppState>) -> Response {
    match sqlx::query("select 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response(),
        Err(err) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "status": "error", "message": err.to_string() })),
        )
            .into_response(),
    }
}
