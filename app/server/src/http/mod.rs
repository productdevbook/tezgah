//! Route assembly.
//!
//! `tests/reachable.rs` in the crate root keeps every domain function honest
//! about having *a* route; `tezgah::api::routes()` names 486 of them. This
//! binary binds a fraction, by hand, and says so out loud at startup rather
//! than leaving the rest to be discovered by a 404: see [`router`]'s doc
//! comment for exactly which, and why those.

pub mod admin;
pub mod auth;
pub mod docs;
pub mod files;
pub mod health;
pub mod store;
pub mod webhook;

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
    /// The shared secret, when there is one. `None` no longer means the admin
    /// surface is unmounted: an installation with operators has a way in
    /// without it — see [`router`].
    pub admin_token: Option<Arc<str>>,
    /// Whether any operator account exists. Read once at startup, because it
    /// decides what is mounted rather than what a request is allowed: an
    /// account made while this is running is signed in with on the next
    /// restart, and `main.rs` says so.
    pub has_operators: bool,
    /// The secret a payment provider's callback is signed with. `None` leaves
    /// `POST /webhooks/payments/{provider}` unmounted — a callback endpoint
    /// that believes anybody is worse than one that is not there, because a
    /// provider retries a 404 and says so on its dashboard while an unsigned
    /// endpoint accepts a forged capture quietly.
    pub webhook_secret: Option<Arc<str>>,
    /// `None` when no SMTP was configured. Everything needing a letter checks
    /// and says it cannot rather than reporting one sent.
    pub mailer: Option<crate::mail::Mailer>,
    /// Where the panel lives, for the link in an invitation. Present exactly
    /// when `mailer` is — `config::Config` refuses one without the other.
    pub panel_url: Option<Arc<str>>,
    /// `None` when `TEZGAH_FILE_DIR` is unset. The upload route and the one
    /// that serves a file back are both unmounted then, and the panel goes on
    /// taking a URL somebody else hosts.
    pub files: Option<crate::files::Store>,
}

/// The paths this binary actually serves out of `tezgah::api::routes()`,
/// versus how many that table declares — logged once at startup and never
/// silently different from what `router()` below mounted.
///
/// `health` is counted separately, and so is `own`: `GET /health`, the two
/// file routes and the accounts are this binary's, not tezgah's, and folding
/// any of them into the same tally would inflate the count against a table
/// they were never part of. That is not hypothetical — the file routes did
/// exactly that between the commit that added them and this one.
#[derive(Debug)]
pub struct Bound {
    pub paths: Vec<(&'static str, &'static str)>,
    /// This binary's own, counted apart from the table's.
    pub own: Vec<(&'static str, &'static str)>,
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
        for (method, path) in &self.own {
            println!("  {method:<6} {path}   (this binary's own, not one of the table's)");
        }
        if self.health {
            println!("  plus GET /health, which is this binary's own and not one of the 486");
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
/// The store surface — catalogue, cart, checkout, and now the reads past
/// those five that had their own functions waiting in `src/api/` — is
/// always mounted; `store::router`'s own doc comment says exactly how many
/// of its routes are bound and why some of `tezgah::api`'s storefront
/// routes still are not. The admin surface is mounted only when
/// `state.admin_token` is `Some`; `admin::router`'s own doc comment names
/// what it binds — the panel's screens, their writes, a screen's row edited
/// or deleted wherever `tezgah::api` has the function for it, and the reads
/// past what the panel has a screen for yet — and `../README.md`'s route
/// table carries the full list. Requires that same bearer token: see
/// `admin::router`'s doc comment for why an unset token means no admin
/// surface at all rather than an open
/// one.
pub fn router(state: AppState) -> (Router, Bound) {
    let mut bound = Vec::new();

    let app_base = Router::new()
        .route("/health", get(health::check))
        .merge(docs::router());

    let (store_router, store_bound) = store::router(state.checkout.is_some());
    let mut app = app_base.merge(store_router);
    bound.extend(store_bound);

    let (webhook_router, webhook_bound) = webhook::router(state.webhook_secret.is_some());
    app = app.merge(webhook_router);
    bound.extend(webhook_bound);

    // Reading a file back is open: an image on a storefront is public, and a
    // signed URL for a product photo is ceremony. Uploading one is not, and
    // goes in with the admin surface below.
    //
    // Into `own` rather than `bound`: neither file route is in
    // `tezgah::api::routes()`, so counting them against it would say this
    // binary serves more of the table than it does.
    let mut own = Vec::new();
    let (file_router, file_bound) = files::router(state.files.is_some());
    app = app.merge(file_router);
    own.extend(file_bound);

    // Mounted when there is any way to authenticate at all. Before operators
    // existed that was `ADMIN_TOKEN` alone, and an unset one meant the admin
    // surface did not exist to be reached rather than existing and refusing
    // everybody. Both halves of that still hold: a shop with accounts has a
    // door, and a shop with neither has none.
    if state.admin_token.is_some() || state.has_operators {
        let gate = admin::Gate {
            pool: state.pool.clone(),
            admin_token: state.admin_token.clone(),
        };
        let (admin_router, admin_bound) = admin::router();
        let (upload_router, upload_bound) = files::admin_router(state.files.is_some());
        let admin_router = admin_router.merge(upload_router);
        own.extend(upload_bound);
        // `route_layer`, not `layer`: `layer` also wraps a router's own
        // fallback, and `Router::merge` picks the *other* router's fallback
        // when both are still the untouched default — so a `.layer`'d
        // fallback here would become the merged app's fallback for every
        // unmatched path, not just this router's own. `route_layer` only
        // wraps matched routes, so a path nothing binds still reaches the
        // ordinary 404 rather than this middleware.
        let admin_router =
            admin_router
                .merge(auth::gated_router())
                .route_layer(middleware::from_fn_with_state(
                    gate,
                    admin::require_operator,
                ));
        app = app.merge(admin_router).merge(auth::open_router());
        bound.extend(admin_bound);
    }

    let declared = tezgah::api::routes().len();
    let report = Bound {
        paths: bound,
        own,
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
