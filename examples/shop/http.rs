//! The six routes bound out of tezgah's 483. See `main.rs`'s doc comment for
//! the rest of the table and why these six are the smallest set that carries
//! a shopper from the catalogue to a placed order.
//!
//! Every handler follows the same shape: open a transaction, announce this
//! shop's scope on it — `crate::ports::scoped` does this already, but it is
//! `pub(crate)`, so, like `seed.rs`, this writes the two lines by hand —
//! build a [`Ctx`], call the one `tezgah::api::store` function that already
//! carries the request's permission check and its view type, and commit.
//!
//! The three catalogue-and-cart-opening routes read the caller's own
//! `x-publishable-key` header — `store::list_products`, `get_product` and
//! `create_cart` all take a token, because it is what decides which sales
//! channel's products a storefront may see. The three cart-by-id routes
//! take none: a cart is already scoped by its own id, the same way
//! `tezgah::api::store`'s own functions are — this file reads no header
//! `tezgah::api::store` does not itself ask for.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use sqlx::PgPool;
use tezgah::api::store::{self, AddLineItem, CreateCart};
use tezgah::checkout::Checkout;
use tezgah::id::CartId;
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};
use uuid::Uuid;

use crate::host::ExampleHost;

#[derive(Clone, Debug)]
pub struct AppState {
    pub pool: PgPool,
    pub host: Arc<ExampleHost>,
    pub checkout: Arc<Checkout>,
    pub scope: Scope,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/store/products", get(list_products))
        .route("/store/products/{handle}", get(get_product))
        .route("/store/carts", post(create_cart))
        .route("/store/carts/{id}", get(get_cart))
        .route("/store/carts/{id}/line-items", post(add_line_item))
        .route("/store/carts/{id}/complete", post(complete_cart))
        .with_state(state)
}

#[derive(Debug)]
struct ApiError(tezgah::Error);

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

fn ctx_for(state: &AppState, actor: Actor) -> Ctx<'_> {
    Ctx::new(state.scope, actor, state.host.as_ref() as &dyn Host)
}

const PUBLISHABLE_KEY_HEADER: &str = "x-publishable-key";

fn publishable_key(headers: &HeaderMap) -> Result<&str, ApiError> {
    headers
        .get(PUBLISHABLE_KEY_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            ApiError(tezgah::Error::invalid(
                "send the shop's storefront token as the x-publishable-key header",
            ))
        })
}

async fn begin(pool: &PgPool, scope: Scope) -> Result<Tx<'static>, ApiError> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

async fn list_products(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<tezgah::page::Page<store::ProductView>>, ApiError> {
    let token = publishable_key(&headers)?;
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: Uuid::nil() });
    let page = store::list_products(&mut tx, &ctx, token, store::ListProducts::default()).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_product(
    State(state): State<AppState>,
    Path(handle): Path<String>,
    headers: HeaderMap,
) -> Result<Json<store::ProductView>, ApiError> {
    let token = publishable_key(&headers)?;
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: Uuid::nil() });
    let product = store::get_product(&mut tx, &ctx, token, &handle, None).await?;
    tx.commit().await?;
    Ok(Json(product))
}

async fn create_cart(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateCart>,
) -> Result<Json<store::CartView>, ApiError> {
    let token = publishable_key(&headers)?;
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: Uuid::nil() });
    let cart = store::create_cart(&mut tx, &ctx, token, input).await?;
    tx.commit().await?;
    Ok(Json(cart))
}

async fn get_cart(
    State(state): State<AppState>,
    Path(id): Path<CartId>,
) -> Result<Json<store::CartView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    let cart = store::get_cart(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(cart))
}

async fn add_line_item(
    State(state): State<AppState>,
    Path(id): Path<CartId>,
    Json(input): Json<AddLineItem>,
) -> Result<Json<store::LineItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    let line = store::add_line_item(&mut tx, &ctx, id, input).await?;
    tx.commit().await?;
    Ok(Json(line))
}

async fn complete_cart(
    State(state): State<AppState>,
    Path(id): Path<CartId>,
) -> Result<Json<store::CompletedView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    // `complete_cart` opens its own transactions off `state.pool` for the
    // checkout workflow itself — `tx` here is only the one the ownership
    // check runs in, the same split its own doc comment describes.
    let completed = store::complete_cart(&mut tx, &ctx, id, &state.checkout, &state.pool).await?;
    tx.commit().await?;
    Ok(Json(completed))
}
