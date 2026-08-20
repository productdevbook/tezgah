//! The API's own description, and something that reads it.
//!
//! `GET /openapi.json` is the document `tezgah::api::openapi::document()`
//! generates from the route table — the same one `tests/snapshots/openapi.json`
//! pins, so what a running server describes and what CI reviews cannot drift.
//!
//! `GET /docs` is [Scalar](https://scalar.com) over it. Both are open: the
//! document says which paths exist and what permission each asks, and every
//! one of those paths already refuses an unauthorised caller on its own. A
//! description that needed protecting would mean the protection was the
//! description.

use axum::{
    Json, Router,
    http::header,
    response::{Html, IntoResponse},
    routing::get,
};
use serde_json::Value;

/// Two routes, and neither is one of tezgah's 483 — they describe them.
///
/// Generic over the state it is merged into: it holds none of its own, and
/// the document it answers with comes from the route table rather than from
/// anything a request carries.
pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/openapi.json", get(document))
        .route("/docs", get(reference))
}

async fn document() -> impl IntoResponse {
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(tezgah::api::openapi::document()),
    )
}

/// Scalar from a CDN rather than vendored: the alternative is carrying a
/// megabyte of somebody else's JavaScript in this repository and in every
/// image built from it, and this page is documentation — a shop that cannot
/// reach a CDN loses the reference and nothing else.
async fn reference() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<html>
  <head>
    <title>tezgah API</title>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
  </head>
  <body>
    <div id="app"></div>
    <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference"></script>
    <script>
      // Relative, not `/openapi.json`: this page is served under a prefix
      // as often as not — the panel's nginx puts it at `/api/docs` — and an
      // absolute path there asks the panel for the document instead of the
      // server. The panel is a single-page app, so it answers that with its
      // own index.html and a 200, and the only sign of trouble is Scalar
      // saying the response is not an object.
      Scalar.createApiReference('#app', {
        url: './openapi.json',
        theme: 'default',
      })
    </script>
  </body>
</html>
"#,
    )
}

/// What `document()` answers with, so a caller can count it without parsing.
pub fn described() -> (usize, usize) {
    let doc: Value = tezgah::api::openapi::document();
    let paths = doc["paths"].as_object().map_or(0, |p| p.len());
    let schemas = doc["components"]["schemas"]
        .as_object()
        .map_or(0, |s| s.len());
    (paths, schemas)
}
