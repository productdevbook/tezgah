//! Signing in, signing out, and the accounts that make either possible.
//!
//! This binary's own routes, not part of `tezgah::api::routes()`'s 483 — the
//! crate authenticates nobody and this is the host doing it, so the tally
//! `http::mod` logs at startup does not count them, the same way it does not
//! count `GET /health`.
//!
//! `POST /auth/session` is the only one of these that is open. Everything
//! else sits behind the same gate the admin surface does, which means the
//! first operator is made with `ADMIN_TOKEN` and every one after that by
//! somebody already signed in. There is no invitation and no reset e-mail:
//! this binary has no mailer, and a reset link it cannot send is worse than
//! one it never offered — `ADMIN_TOKEN` is the way back in.

use axum::extract::{Path, State};
use axum::{Extension, Json, Router};
use axum::routing::{get, patch, post};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::http::{ApiError, AppState};
use crate::identity::{self, Operator};

#[derive(Debug, Deserialize)]
pub struct Credentials {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct SessionView {
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub operator: OperatorView,
}

#[derive(Debug, Serialize)]
pub struct OperatorView {
    pub id: Uuid,
    pub email: String,
    pub name: String,
}

impl From<Operator> for OperatorView {
    fn from(operator: Operator) -> Self {
        OperatorView {
            id: operator.id,
            email: operator.email,
            name: operator.name,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OperatorRowView {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct NewOperator {
    pub email: String,
    pub name: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct OperatorPatch {
    pub disabled: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct NewPassword {
    pub password: String,
}

/// The one route that cannot be behind the gate, because it is how somebody
/// gets through it.
pub fn open_router() -> Router<AppState> {
    Router::new().route("/auth/session", post(sign_in))
}

/// Everything else, mounted behind `admin::require_operator` by
/// `http::router` — the same middleware the admin surface uses, so an
/// `ADMIN_TOKEN` holder can make the first account and nobody else can.
pub fn gated_router() -> Router<AppState> {
    Router::new()
        .route("/auth/session", axum::routing::delete(sign_out))
        .route("/auth/me", get(me))
        .route("/auth/password", post(set_own_password))
        .route("/admin/operators", get(list).post(create))
        .route("/admin/operators/{id}", patch(update))
}

async fn sign_in(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<SessionView>, ApiError> {
    let issued = identity::sign_in(&state.pool, &body.email, &body.password).await?;
    Ok(Json(SessionView {
        token: issued.token,
        expires_at: issued.expires_at,
        operator: issued.operator.into(),
    }))
}

async fn sign_out(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Caller::Session { token, .. } = &caller {
        identity::sign_out(&state.pool, token).await?;
    }
    Ok(Json(serde_json::json!({ "signed_out": true })))
}

/// Who the caller is. `null` for an `ADMIN_TOKEN` holder, which is the honest
/// answer: the shared secret is not a person.
async fn me(Extension(caller): Extension<Caller>) -> Json<Option<OperatorView>> {
    Json(caller.operator().map(|operator| operator.clone().into()))
}

async fn set_own_password(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<NewPassword>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Caller::Session { operator, token } = &caller else {
        return Err(tezgah::Error::invalid(
            "an ADMIN_TOKEN holder has no password to change",
        )
        .into());
    };

    identity::change_password(&state.pool, operator.id, &body.password, Some(token.as_str()))
        .await?;
    Ok(Json(serde_json::json!({ "changed": true })))
}

async fn list(State(state): State<AppState>) -> Result<Json<Vec<OperatorRowView>>, ApiError> {
    let rows = identity::list_operators(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| OperatorRowView {
                id: row.id,
                email: row.email,
                name: row.name,
                created_at: row.created_at,
                disabled_at: row.disabled_at,
            })
            .collect(),
    ))
}

async fn create(
    State(state): State<AppState>,
    Json(body): Json<NewOperator>,
) -> Result<Json<OperatorView>, ApiError> {
    let made =
        identity::create_operator(&state.pool, &body.email, &body.name, &body.password).await?;
    Ok(Json(made.into()))
}

async fn update(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<Uuid>,
    Json(body): Json<OperatorPatch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(disabled) = body.disabled {
        // Disabling yourself locks the door with the key inside. `ADMIN_TOKEN`
        // would still open it, but a deployment that unset it after making
        // accounts would have nothing left.
        if disabled && caller.operator().map(|operator| operator.id) == Some(id) {
            return Err(tezgah::Error::invalid("an operator cannot disable itself").into());
        }
        identity::set_disabled(&state.pool, id, disabled).await?;
    }
    Ok(Json(serde_json::json!({ "updated": true })))
}

/// What cleared the gate, kept on the request so a handler can tell the two
/// apart — and so `ctx_for` can name a person in the audit row rather than a
/// nil uuid.
#[derive(Debug, Clone)]
pub enum Caller {
    /// Somebody signed in, and the token they are holding.
    Session { operator: Operator, token: String },
    /// The shared secret. Not a person, and says so.
    AdminToken,
}

impl Caller {
    pub fn operator(&self) -> Option<&Operator> {
        match self {
            Caller::Session { operator, .. } => Some(operator),
            Caller::AdminToken => None,
        }
    }

    /// What tezgah is told. An `ADMIN_TOKEN` request has no person behind it,
    /// so it carries the nil uuid it always carried — visibly one value rather
    /// than a made-up identity, and `Actor::Staff` either way because a shared
    /// secret is still the back office rather than the system.
    pub fn actor_id(&self) -> Uuid {
        self.operator().map(|operator| operator.id).unwrap_or(Uuid::nil())
    }
}
