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

use axum::extract::{Path, Query, State};
use axum::routing::{get, patch, post};
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::http::{ApiError, AppState};
use crate::identity::{self, Operator, Role};

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
    pub role: &'static str,
}

impl From<Operator> for OperatorView {
    fn from(operator: Operator) -> Self {
        OperatorView {
            id: operator.id,
            email: operator.email,
            name: operator.name,
            role: operator.role.as_str(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OperatorRowView {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: &'static str,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct NewOperator {
    pub email: String,
    pub name: String,
    pub password: String,
    /// `owner`, `staff` or `viewer`. Anything else reads as `viewer` — the
    /// narrowest, because a typo should close the door rather than open it.
    /// The first account made is the owner whatever this says.
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OperatorPatch {
    pub disabled: Option<bool>,
    pub role: Option<String>,
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
        .route("/admin/operators/{id}/password", post(reset_password))
        .route("/admin/records/audit", get(audit))
        .route("/admin/records/events", get(events))
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
        return Err(
            tezgah::Error::invalid("an ADMIN_TOKEN holder has no password to change").into(),
        );
    };

    identity::change_password(
        &state.pool,
        operator.id,
        &body.password,
        Some(token.as_str()),
    )
    .await?;
    Ok(Json(serde_json::json!({ "changed": true })))
}

/// Reading who else is here is not owner-only: an operator who cannot see the
/// accounts cannot tell whether the person asking them for something has one.
async fn list(State(state): State<AppState>) -> Result<Json<Vec<OperatorRowView>>, ApiError> {
    let rows = identity::list_operators(&state.pool).await?;
    Ok(Json(
        rows.into_iter()
            .map(|row| OperatorRowView {
                id: row.id,
                email: row.email,
                name: row.name,
                role: row.role.as_str(),
                created_at: row.created_at,
                disabled_at: row.disabled_at,
            })
            .collect(),
    ))
}

async fn create(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<NewOperator>,
) -> Result<Json<OperatorView>, ApiError> {
    only_an_owner(&caller)?;

    let role = body.role.as_deref().map_or(Role::Staff, Role::parse);
    let made =
        identity::create_operator(&state.pool, &body.email, &body.name, &body.password, role)
            .await?;
    Ok(Json(made.into()))
}

/// An owner setting somebody else's password.
///
/// This is what a shop does when an operator forgets theirs, and it is why
/// there is no reset e-mail: an owner sets a new one and tells them the way
/// they told them the first one. A link this server cannot send would be
/// worse than one it never offered.
///
/// Every session that operator holds ends with it — including, deliberately,
/// the one they may be sitting in. An account whose password was reset by
/// somebody else is an account that may have been taken.
async fn reset_password(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<Uuid>,
    Json(body): Json<NewPassword>,
) -> Result<Json<serde_json::Value>, ApiError> {
    only_an_owner(&caller)?;
    identity::change_password(&state.pool, id, &body.password, None).await?;
    Ok(Json(serde_json::json!({ "changed": true })))
}

/// `ADMIN_TOKEN` counts as an owner, and has to: it is how the first account
/// is made, and how a shop that lost every owner's password makes another.
fn only_an_owner(caller: &Caller) -> Result<(), ApiError> {
    let allowed = match caller {
        Caller::AdminToken => true,
        Caller::Session { operator, .. } => operator.role.may_manage_operators(),
    };

    if allowed {
        Ok(())
    } else {
        Err(tezgah::Error::denied().into())
    }
}

async fn update(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<Uuid>,
    Json(body): Json<OperatorPatch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    only_an_owner(&caller)?;

    if let Some(role) = body.role.as_deref().map(Role::parse) {
        // The last owner cannot be demoted, for the same reason the first
        // account is made one: a shop with no owner cannot make an account,
        // and the only way back is the token it was told it could stop
        // keeping.
        let is_owner_now = identity::list_operators(&state.pool)
            .await?
            .into_iter()
            .any(|row| row.id == id && row.role == Role::Owner);

        if is_owner_now && role != Role::Owner && identity::owners(&state.pool).await? <= 1 {
            return Err(tezgah::Error::invalid(
                "that is the last owner — make another before narrowing this one",
            )
            .into());
        }

        identity::set_role(&state.pool, id, role).await?;
    }

    if let Some(disabled) = body.disabled {
        // Disabling yourself locks the door with the key inside. `ADMIN_TOKEN`
        // would still open it, but a deployment that unset it after making
        // accounts would have nothing left.
        if disabled && caller.operator().map(|operator| operator.id) == Some(id) {
            return Err(tezgah::Error::invalid("an operator cannot disable itself").into());
        }
        if disabled && identity::owners(&state.pool).await? <= 1 {
            let last_owner = identity::list_operators(&state.pool)
                .await?
                .into_iter()
                .any(|row| row.id == id && row.role == Role::Owner);
            if last_owner {
                return Err(tezgah::Error::invalid(
                    "that is the last owner — make another before disabling this one",
                )
                .into());
            }
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
        self.operator()
            .map(|operator| operator.id)
            .unwrap_or(Uuid::nil())
    }
}

#[derive(Debug, Deserialize)]
pub struct Recent {
    /// How many rows, clamped the same way a page is: a screen asking for a
    /// hundred thousand wants as many as it can have, not an error.
    pub limit: Option<i64>,
}

impl Recent {
    fn rows(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, 200)
    }
}

#[derive(Debug, Serialize)]
pub struct AuditRow {
    pub id: Uuid,
    pub actor_kind: String,
    pub actor_id: Option<Uuid>,
    pub action: String,
    pub entity: String,
    pub entity_id: Uuid,
    pub summary: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub struct EventRow {
    pub id: Uuid,
    pub name: String,
    pub entity_id: Uuid,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
}

type AuditTuple = (
    Uuid,
    String,
    Option<Uuid>,
    String,
    String,
    Uuid,
    serde_json::Value,
    chrono::DateTime<chrono::Utc>,
);

type EventTuple = (
    Uuid,
    String,
    Uuid,
    serde_json::Value,
    chrono::DateTime<chrono::Utc>,
    Option<chrono::DateTime<chrono::Utc>>,
);

/// What was written down, newest first.
///
/// Not one of tezgah's routes — the crate asks a host to keep an audit trail
/// and does not say how it is read back. Newest first and a fixed ceiling
/// rather than a cursor, because what this answers is "what just happened",
/// and a shop asking a longer question wants the database rather than a
/// screen.
async fn audit(
    State(state): State<AppState>,
    Query(query): Query<Recent>,
) -> Result<Json<Vec<AuditRow>>, ApiError> {
    let rows: Vec<AuditTuple> = sqlx::query_as(
        "select id, actor_kind, actor_id, action, entity, entity_id, summary, created_at
         from server_audit
         order by created_at desc, id desc
         limit $1",
    )
    .bind(query.rows())
    .fetch_all(&state.pool)
    .await
    .map_err(tezgah::Error::from)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, actor_kind, actor_id, action, entity, entity_id, summary, created_at)| {
                    AuditRow {
                        id,
                        actor_kind,
                        actor_id,
                        action,
                        entity,
                        entity_id,
                        summary,
                        created_at,
                    }
                },
            )
            .collect(),
    ))
}

/// The outbox, newest first. `delivered_at` is null on every row, because
/// nothing here sends them anywhere — see `host::ServerHost`'s `emit`.
async fn events(
    State(state): State<AppState>,
    Query(query): Query<Recent>,
) -> Result<Json<Vec<EventRow>>, ApiError> {
    let rows: Vec<EventTuple> = sqlx::query_as(
        "select id, name, entity_id, payload, created_at, delivered_at
         from server_event
         order by created_at desc, id desc
         limit $1",
    )
    .bind(query.rows())
    .fetch_all(&state.pool)
    .await
    .map_err(tezgah::Error::from)?;

    Ok(Json(
        rows.into_iter()
            .map(
                |(id, name, entity_id, payload, created_at, delivered_at)| EventRow {
                    id,
                    name,
                    entity_id,
                    payload,
                    created_at,
                    delivered_at,
                },
            )
            .collect(),
    ))
}
