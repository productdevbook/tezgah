//! The admin surface: one list endpoint per screen the panel in `client/`
//! draws — products, orders, inventory items, customers, promotions,
//! subscriptions, and the two the store screen's tabs read, regions and
//! sales channels — plus the currencies list the overview screen reads.
//! Nine routes out of 483 declared; every write, every fetch-by-id and
//! everything the other domains in `tezgah::api` offer stays unbound. What
//! is here is what the shipped panel needs to render its seven screens and
//! nothing beyond that was chosen for this binary specifically.
//!
//! # Why a bearer token, and why it is the whole of this
//!
//! `docs/hosting.md` and `tezgah::ports::Authorizer` are explicit that
//! tezgah authenticates nobody — a host supplies its own roles, or, as
//! `ServerHost` does, supplies none and grants every actor. A production
//! server cannot leave the back office at that. It also cannot invent a
//! second role system on tezgah's behalf without becoming exactly the
//! "second set of roles" the crate's own docs say a host should not be
//! handed. `ADMIN_TOKEN` is the middle: one shared secret, checked here, in
//! front of every route this module binds — and when it is not set, `Bound`
//! in `http::mod` never mounts this router at all, so the admin surface does
//! not exist to be reached rather than existing and refusing everyone. A
//! deployment that wants real operators with real identities replaces this
//! middleware; nothing downstream of it needs to change to allow that.

use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use subtle::ConstantTimeEq;
use tezgah::api::{admin_catalogue, admin_order, admin_rest, subscription};
use tezgah::ports::{Actor, Ctx, Host};
use uuid::Uuid;

use super::{ApiError, AppState, begin};

pub fn router() -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    let bound = vec![
        ("GET", "/admin/products"),
        ("GET", "/admin/orders"),
        ("GET", "/admin/inventory-items"),
        ("GET", "/admin/customers"),
        ("GET", "/admin/promotions"),
        ("GET", "/admin/subscriptions"),
        ("GET", "/admin/regions"),
        ("GET", "/admin/sales-channels"),
        ("GET", "/admin/currencies"),
    ];

    let router = Router::new()
        .route("/admin/products", get(list_products))
        .route("/admin/orders", get(list_orders))
        .route("/admin/inventory-items", get(list_inventory_items))
        .route("/admin/customers", get(list_customers))
        .route("/admin/promotions", get(list_promotions))
        .route("/admin/subscriptions", get(list_subscriptions))
        .route("/admin/regions", get(list_regions))
        .route("/admin/sales-channels", get(list_sales_channels))
        .route("/admin/currencies", get(list_currencies));

    (router, bound)
}

/// This binary models no individual operators — one token speaks for the
/// whole back office, so every request that clears `require_token` runs as
/// the same nil-uuid `Actor::Staff`. A host that tells its operators apart
/// authenticates them before this point and sets a real id here.
fn ctx_for(state: &AppState) -> Ctx<'_> {
    Ctx::new(
        state.scope,
        Actor::Staff { id: Uuid::nil() },
        state.host.as_ref() as &dyn Host,
    )
}

/// Checked in constant time so a byte-by-byte timing difference cannot be
/// used to guess the token — the same discipline tezgah's own webhook
/// signature checks use `subtle` for.
pub async fn require_token(
    State(expected): State<Arc<str>>,
    request: Request,
    next: Next,
) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let authorized = match presented {
        Some(token) if token.len() == expected.len() => {
            bool::from(token.as_bytes().ct_eq(expected.as_bytes()))
        }
        _ => false,
    };

    if authorized {
        next.run(request).await
    } else {
        denied()
    }
}

fn denied() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": {
                "code": "denied",
                "message": "send the admin token as \"authorization: Bearer <token>\"",
            }
        })),
    )
        .into_response()
}

async fn list_products(
    State(state): State<AppState>,
    Query(query): Query<admin_catalogue::ListProducts>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::ProductView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_catalogue::list_products(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_orders(
    State(state): State<AppState>,
    Query(query): Query<admin_order::ListOrders>,
) -> Result<Json<tezgah::page::Page<admin_order::OrderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_orders(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_inventory_items(
    State(state): State<AppState>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::InventoryItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_catalogue::list_inventory_items(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_customers(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::CustomerView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_customers(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_promotions(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::PromotionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_promotions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_subscriptions(
    State(state): State<AppState>,
    Query(query): Query<subscription::List>,
) -> Result<Json<tezgah::page::Page<subscription::SubscriptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = subscription::list_subscriptions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_regions(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::RegionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_regions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_sales_channels(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::SalesChannelView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_sales_channels(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_currencies(
    State(state): State<AppState>,
) -> Result<Json<Vec<admin_rest::CurrencyView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let currencies = admin_rest::list_currencies(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(currencies))
}
