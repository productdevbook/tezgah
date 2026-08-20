//! The storefront: catalogue, cart, and — when `state.checkout` is `Some` —
//! checkout.
//!
//! Nine routes when a stock location is configured, eight without — a
//! browser walking catalogue to order needs exactly this shape:
//! `examples/shop` walks the same five calls (plus checkout) directly,
//! without a router at all, to show what these look like as plain library
//! calls. The three catalogue-and-cart-opening routes read the caller's own
//! `x-publishable-key` header — it decides which sales channel's products a
//! storefront may see. The two cart-by-id routes ahead of checkout take
//! none: a cart is already scoped by its own id. `GET /store/shipping-options`
//! and `GET /store/payment-providers` take neither — the cart each prices
//! delivery for, or narrows a provider list to the region of, is named in
//! its own query, the same as `own_cart` asks the host about for every
//! other route here. `GET /store/carts/{id}/credits` takes only the id,
//! the same as the two cart-by-id routes above it.
//!
//! # What this surface still cannot reach
//!
//! `tezgah::api` declares four more storefront routes this binary does not
//! bind: `GET /store/customers/me/store-credit`
//! (`credit::my_store_credit`), and digital's `GET /store/entitlements`,
//! `POST /store/entitlements/{id}/token` and `POST /store/downloads`
//! (`digital::my_entitlements`, `create_token`, `redeem`) — every one of
//! them calls `signed_in` before anything else, which succeeds only for
//! `Actor::Customer`. Every route this file binds runs as `Actor::Guest`:
//! this binary has no customer sign-in anywhere. Binding one of these four
//! would mean a route that answers `denied` to every caller it could ever
//! have, which is worse than leaving it unbound — giving this binary a
//! customer identity is a separate decision, the same shape as #214.

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use tezgah::api::credit;
use tezgah::api::store::{self, AddLineItem, CreateCart};
use tezgah::id::CartId;
use tezgah::ports::{Actor, Ctx, Host};
use uuid::Uuid;

use super::{ApiError, AppState, begin};

/// `checkout_configured` decides whether `POST /store/carts/{id}/complete`
/// is mounted at all — see this module's own doc comment for why an
/// unconfigured checkout is left unbound rather than bound and answering
/// with an error on every call.
pub fn router(checkout_configured: bool) -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    let mut bound = vec![
        ("GET", "/store/products"),
        ("GET", "/store/products/{handle}"),
        ("POST", "/store/carts"),
        ("GET", "/store/carts/{id}"),
        ("POST", "/store/carts/{id}/line-items"),
        ("GET", "/store/shipping-options"),
        ("GET", "/store/payment-providers"),
        ("GET", "/store/carts/{id}/credits"),
    ];

    let mut router = Router::new()
        .route("/store/products", get(list_products))
        .route("/store/products/{handle}", get(get_product))
        .route("/store/carts", post(create_cart))
        .route("/store/carts/{id}", get(get_cart))
        .route("/store/carts/{id}/line-items", post(add_line_item))
        .route("/store/shipping-options", get(list_shipping_options))
        .route("/store/payment-providers", get(list_payment_providers))
        .route("/store/carts/{id}/credits", get(list_cart_credits));

    if checkout_configured {
        router = router.route("/store/carts/{id}/complete", post(complete_cart));
        bound.push(("POST", "/store/carts/{id}/complete"));
    }

    (router, bound)
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
    let Some(checkout) = state.checkout.as_ref() else {
        return Err(ApiError(tezgah::Error::invalid(
            "checkout is not configured on this server — set TEZGAH_STOCK_LOCATION_ID",
        )));
    };
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    // `complete_cart` opens its own transactions off `state.pool` for the
    // checkout workflow itself — `tx` here is only the one the ownership
    // check runs in, the same split its own doc comment describes.
    let completed = store::complete_cart(&mut tx, &ctx, id, checkout, &state.pool).await?;
    tx.commit().await?;
    Ok(Json(completed))
}

async fn list_shipping_options(
    State(state): State<AppState>,
    Query(query): Query<store::ListShippingOptions>,
) -> Result<Json<Vec<store::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(
        &state,
        Actor::Guest {
            cart: query.cart_id.as_uuid(),
        },
    );
    let options = store::list_shipping_options(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(options))
}

async fn list_payment_providers(
    State(state): State<AppState>,
    Query(query): Query<store::ListPaymentProviders>,
) -> Result<Json<Vec<store::PaymentProviderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(
        &state,
        Actor::Guest {
            cart: query.cart_id.as_uuid(),
        },
    );
    let providers = store::list_payment_providers(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(providers))
}

async fn list_cart_credits(
    State(state): State<AppState>,
    Path(id): Path<CartId>,
) -> Result<Json<Vec<credit::CartCreditView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    let credits = credit::list_cart_credits(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(credits))
}
