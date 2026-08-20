//! Route assembly.
//!
//! `tests/reachable.rs` in the crate root keeps every domain function honest
//! about having *a* route; `tezgah::api::routes()` names 483 of them. This
//! binary binds a fraction, by hand, and says so out loud at startup rather
//! than leaving the rest to be discovered by a 404: see [`router`]'s doc
//! comment for exactly which, and why those.

pub mod admin;
pub mod docs;
pub mod health;
pub mod store;

use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Router, middleware};
use sqlx::PgPool;
use tezgah::checkout::Checkout;
use tezgah::ports::{Scope, Tx};

use crate::host::ServerHost;

#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub host: Arc<ServerHost>,
    /// `None` when `TEZGAH_STOCK_LOCATION_ID` was not set, or when
    /// `TEZGAH_DEMO_BANK` was not set to the phrase `config::Config` requires
    /// — `store::router` does not mount `POST /store/carts/{id}/complete` in
    /// either case: the first because `tezgah::checkout::Checkout::new` needs
    /// a location and there is none to give it, the second because the only
    /// payment provider this binary can build it with is a demo that takes
    /// no real money.
    pub checkout: Option<Arc<Checkout>>,
    pub scope: Scope,
    /// `None` means the admin surface is not mounted — see
    /// `admin::router`'s doc comment.
    pub admin_token: Option<Arc<str>>,
}

/// The paths this binary actually serves out of `tezgah::api::routes()`,
/// versus how many that table declares — logged once at startup and never
/// silently different from what `router()` below mounted. `health` is
/// counted separately: `GET /health` is this binary's own, not one of
/// tezgah's 483, and folding it into the same tally would inflate the count
/// against a table it was never part of.
#[derive(Debug)]
pub struct Bound {
    pub paths: Vec<(&'static str, &'static str)>,
    pub declared: usize,
    pub health: bool,
}

impl Bound {
    pub fn log(&self) {
        println!(
            "bound {} of {} declared routes",
            self.paths.len(),
            self.declared
        );
        for (method, path) in &self.paths {
            println!("  {method:<6} {path}");
        }
        if self.health {
            println!("  plus GET /health, which is this binary's own and not one of the 483");
            let (paths, schemas) = docs::described();
            println!(
                "  plus GET /openapi.json and GET /docs, describing {paths} paths \
                 and {schemas} schemas — also this binary's own"
            );
        }
    }
}

/// Assembles the router and reports what it bound.
///
/// The store surface — catalogue, cart, checkout — is always mounted;
/// `store::router`'s own doc comment says which six or five of those routes
/// depending on whether checkout is configured. The admin surface is mounted
/// only when `state.admin_token` is `Some`, and everything under it — eleven
/// list endpoints the panel in `client/` reads, the eight single-row reads
/// behind a click on one of those lists, the twelve writes a fresh install
/// needs to fill a shop, a promotion's own edit and delete, and the reads
/// past what the panel has a screen for yet (`admin::router`'s own doc
/// comment names all five) — requires that same bearer token: see
/// `admin::router`'s doc comment for why an unset token means no admin
/// surface at all rather than an open one.
pub fn router(state: AppState) -> (Router, Bound) {
    let mut bound = Vec::new();

    let app_base = Router::new()
        .route("/health", get(health::check))
        .merge(docs::router());

    let (store_router, store_bound) = store::router(state.checkout.is_some());
    let mut app = app_base.merge(store_router);
    bound.extend(store_bound);

    if let Some(token) = state.admin_token.clone() {
        let (admin_router, admin_bound) = admin::router();
        // `route_layer`, not `layer`: `layer` also wraps a router's own
        // fallback, and `Router::merge` picks the *other* router's fallback
        // when both are still the untouched default — so a `.layer`'d
        // fallback here would become the merged app's fallback for every
        // unmatched path, not just this router's own. `route_layer` only
        // wraps matched routes, so a path nothing binds still reaches the
        // ordinary 404 rather than this middleware.
        let admin_router =
            admin_router.route_layer(middleware::from_fn_with_state(token, admin::require_token));
        app = app.merge(admin_router);
        bound.extend(admin_bound);
    }

    let declared = tezgah::api::routes().len();
    let report = Bound {
        paths: bound,
        declared,
        health: true,
    };

    (app.with_state(state), report)
}

#[derive(Debug)]
pub struct ApiError(tezgah::Error);

impl From<tezgah::Error> for ApiError {
    fn from(err: tezgah::Error) -> Self {
        ApiError(err)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError(tezgah::Error::from(err))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // `Error::code` is the stable string this maps on; `Error::report`
        // — which can name a table or a constraint — never leaves this
        // process, per its own doc.
        let status = match self.0.code() {
            "invalid" => StatusCode::BAD_REQUEST,
            "not_found" => StatusCode::NOT_FOUND,
            "denied" => StatusCode::FORBIDDEN,
            "conflict" | "out_of_stock" => StatusCode::CONFLICT,
            "provider" => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if self.0.is_internal() {
            eprintln!("internal error: {}", self.0.report());
        }
        let body = Json(serde_json::json!({
            "error": { "code": self.0.code(), "message": self.0.to_string() },
        }));
        (status, body).into_response()
    }
}

/// Opens a transaction and announces this server's one scope on it —
/// `crate::ports::scoped` does exactly this inside the crate, but it is
/// `pub(crate)`, so, like `examples/shop`, this writes the two lines by hand.
pub(crate) async fn begin(pool: &PgPool, scope: Scope) -> Result<Tx<'static>, ApiError> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
