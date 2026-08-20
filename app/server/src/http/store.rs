//! The storefront: catalogue, cart, and — when `state.checkout` is `Some` —
//! checkout.
//!
//! Sixteen routes when a stock location is configured, fifteen without — a
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
//! # Who is asking
//!
//! Until there was a sign-in here, every route on this surface ran as
//! `Actor::Guest` with the cart id read out of the URL — so four declared
//! routes were left unbound, because each calls `signed_in` first and would
//! have answered `denied` to every caller it could ever have.
//!
//! `crate::shopper` is the credential half tezgah does not have: a `customer`
//! row with `has_account` is the crate's, and the password beside it is this
//! binary's. A storefront that holds a session token sends it as a bearer,
//! the `Shopper` extractor turns it into `Actor::Customer`, and three of
//! those routes are bound.
//!
//! The catalogue and cart routes still run as a guest and still take no
//! token, which is the point of a guest cart: a cart id is the credential,
//! and a shopper who signs in later carries theirs over through
//! `POST /store/carts/{id}/customer`.
//!
//! What this surface still does not reach: `GET /store/customers/me/store-credit`
//! and digital's `GET /store/entitlements`, `POST /store/entitlements/{id}/token`
//! and `POST /store/downloads`. Each is now bindable rather than pointless —
//! there is an actor for them — and each is its own decision about what a
//! storefront should be able to download.

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
        ("GET", "/store/carts/{id}/line-items"),
        ("GET", "/store/customers/me"),
        ("GET", "/store/customers/me/addresses"),
        ("GET", "/store/orders"),
    ];

    let mut router = Router::new()
        .route("/store/products", get(list_products))
        .route("/store/products/{handle}", get(get_product))
        .route("/store/carts", post(create_cart))
        .route("/store/carts/{id}", get(get_cart))
        .route(
            "/store/carts/{id}/line-items",
            get(list_line_items).post(add_line_item),
        )
        .route("/store/shipping-options", get(list_shipping_options))
        .route("/store/payment-providers", get(list_payment_providers))
        .route("/store/carts/{id}/credits", get(list_cart_credits))
        .route("/store/auth/register", post(register))
        .route("/store/auth/session", post(sign_in).delete(sign_out))
        .route("/store/customers/me", get(me))
        .route("/store/customers/me/addresses", get(my_addresses))
        .route("/store/orders", get(my_orders));

    if checkout_configured {
        router = router.route("/store/carts/{id}/complete", post(complete_cart));
        bound.push(("POST", "/store/carts/{id}/complete"));
    }

    (router, bound)
}

fn ctx_for(state: &AppState, actor: Actor) -> Ctx<'_> {
    Ctx::new(state.scope, actor, state.host.as_ref() as &dyn Host)
}

/// Whoever is signed in, out of the bearer token a storefront sends.
///
/// An extractor rather than a layer over the whole surface: most of these
/// routes are a catalogue or a cart and have no shopper, and a layer would
/// have to let them through anyway. A route that needs one says so by asking
/// for this, and a route that does not cannot accidentally read it.
///
/// The refusal is `denied` and not `unauthenticated`, matching what the crate
/// answers a `signed_in` that fails: the storefront should not learn from a
/// status code whether an address is registered.
struct Shopper(Uuid);

impl axum::extract::FromRequestParts<AppState> for Shopper {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or_else(|| ApiError(tezgah::Error::denied()))?;

        match crate::shopper::session_customer(&state.pool, token).await? {
            Some(id) => Ok(Shopper(id)),
            None => Err(ApiError(tezgah::Error::denied())),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(Debug, serde::Deserialize)]
struct Registration {
    email: String,
    password: String,
    first_name: Option<String>,
    last_name: Option<String>,
    phone: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct ShopperSession {
    token: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    customer_id: Uuid,
}

/// Two writes and they are not one transaction: the `customer` row is
/// tezgah's and the password is this binary's, in a table tezgah owns no
/// migration for.
///
/// So the address is checked first and the credential is written second. A
/// failure between them leaves a customer with no password — an account
/// somebody can be given one for — rather than a password naming a customer
/// that does not exist, which nothing could repair.
async fn register(
    State(state): State<AppState>,
    Json(body): Json<Registration>,
) -> Result<Json<ShopperSession>, ApiError> {
    if crate::shopper::taken(&state.pool, &body.email).await? {
        return Err(ApiError(tezgah::Error::conflict(
            "somebody already shops with that e-mail address",
        )));
    }

    let mut tx = begin(&state.pool, state.scope).await?;
    // `Actor::System`: nobody is signed in yet, and the shopper being made is
    // not yet somebody who could ask for this.
    let ctx = ctx_for(&state, Actor::System);
    let made = store::create_customer(
        &mut tx,
        &ctx,
        store::CreateCustomer {
            email: body.email.clone(),
            first_name: body.first_name,
            last_name: body.last_name,
            phone: body.phone,
            company_name: None,
        },
    )
    .await?;
    tx.commit().await?;

    crate::shopper::attach_credential(&state.pool, made.id, &body.email, &body.password).await?;

    let issued = crate::shopper::sign_in(&state.pool, &body.email, &body.password).await?;
    Ok(Json(ShopperSession {
        token: issued.token,
        expires_at: issued.expires_at,
        customer_id: issued.customer_id,
    }))
}

async fn sign_in(
    State(state): State<AppState>,
    Json(body): Json<Credentials>,
) -> Result<Json<ShopperSession>, ApiError> {
    let issued = crate::shopper::sign_in(&state.pool, &body.email, &body.password).await?;
    Ok(Json(ShopperSession {
        token: issued.token,
        expires_at: issued.expires_at,
        customer_id: issued.customer_id,
    }))
}

async fn sign_out(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Some(token) = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
    {
        crate::shopper::sign_out(&state.pool, token).await?;
    }
    Ok(Json(serde_json::json!({ "signed_out": true })))
}

async fn me(
    State(state): State<AppState>,
    Shopper(who): Shopper,
) -> Result<Json<store::CustomerView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Customer { id: who });
    let customer = store::me(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(customer))
}

async fn my_addresses(
    State(state): State<AppState>,
    Shopper(who): Shopper,
    Query(query): Query<store::ListPage>,
) -> Result<Json<tezgah::page::Page<store::AddressView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Customer { id: who });
    let page = store::list_my_addresses(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn my_orders(
    State(state): State<AppState>,
    Shopper(who): Shopper,
    Query(query): Query<store::ListPage>,
) -> Result<Json<tezgah::page::Page<store::OrderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Customer { id: who });
    let page = store::list_my_orders(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
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

async fn list_line_items(
    State(state): State<AppState>,
    Path(id): Path<CartId>,
) -> Result<Json<Vec<store::LineItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, Actor::Guest { cart: id.as_uuid() });
    let lines = store::list_line_items(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(lines))
}
