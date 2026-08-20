//! The admin surface: one list endpoint per screen the panel in `client/`
//! draws, the single-row read behind a click on any of them, the twelve
//! writes a fresh install needs to reach its first order (#214), and, past
//! the panel, every list-and-single-read a domain already had the
//! functions for in `src/api/` with nothing here calling them —
//! order-basket, workflow, payout, fulfilment, tax, pricing, payment,
//! credit and (list, single read and writes both, because the domain had
//! no route at all) digital. `../README.md`'s own route table carries the
//! full breakdown, kept there rather than duplicated here because it moves
//! with every domain this binary picks up next. Everything else
//! `tezgah::api` offers stays unbound; nothing here was chosen for this
//! binary beyond what those needs cover.
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
//!
//! # One token still gates both reads and writes
//!
//! `ADMIN_TOKEN` draws no line between them: whatever bearer clears
//! `require_token` reaches `ctx_for` as the same `Actor::Staff`, and
//! `ServerHost::authorize` grants every `tezgah::ports::Action` to it,
//! `View` and `Write` and `Delete` alike. Before this change that was
//! true of nine read routes; now it is true of a token that can also mint a
//! publishable key or create a product. `tezgah::ports::Authorizer::authorize`
//! already receives the `Action` on every call — that is the seam a split
//! would use: a second token (or a role carried on the request, set by
//! `require_token`) read here and turned into which `Action`s a `ServerHost`
//! grants, denying `Write`/`Delete`/`Settle`/`Moderate` for a request that
//! authenticated with a read-only credential. Left as one token for now,
//! matching what this binary shipped with; #214 raises the question rather
//! than answering it.

use std::sync::Arc;

use axum::extract::{Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use subtle::ConstantTimeEq;
use tezgah::api::{
    admin_catalogue, admin_order, admin_rest, credit, digital, order_basket, payout,
    store as store_api, subscription, tax_identity,
};
use tezgah::id::{
    CustomerId, DigitalContentId, FulfillmentId, FulfillmentSetId, GiftCardId, InventoryItemId,
    OrderBasketId, OrderId, PaymentCollectionId, PaymentId, PriceId, PriceListId, PriceSetId,
    ProductId, PromotionId, RegionId, SalesChannelId, ShippingOptionId, ShippingProfileId,
    StoreCreditId, SubscriptionId, TaxRateId, TaxRegionId, VariantId, WorkflowRunId,
};
use tezgah::ports::{Actor, Ctx, Host};
use uuid::Uuid;

use super::{ApiError, AppState, begin};

pub fn router() -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    let bound = vec![
        ("GET", "/admin/products"),
        ("GET", "/admin/products/{id}"),
        ("GET", "/admin/orders"),
        ("GET", "/admin/orders/{id}"),
        ("GET", "/admin/inventory-items"),
        ("GET", "/admin/inventory-items/{id}"),
        ("GET", "/admin/customers"),
        ("GET", "/admin/customers/{id}"),
        ("GET", "/admin/promotions"),
        ("GET", "/admin/promotions/{id}"),
        ("GET", "/admin/subscriptions"),
        ("GET", "/admin/subscriptions/{id}"),
        ("GET", "/admin/regions"),
        ("GET", "/admin/regions/{id}"),
        ("GET", "/admin/sales-channels"),
        ("GET", "/admin/sales-channels/{id}"),
        ("GET", "/admin/currencies"),
        ("GET", "/admin/publishable-api-keys"),
        ("GET", "/admin/stock-locations"),
        ("POST", "/admin/currencies"),
        ("POST", "/admin/regions"),
        ("POST", "/admin/sales-channels"),
        ("POST", "/admin/publishable-api-keys"),
        ("POST", "/admin/stock-locations"),
        ("POST", "/admin/products"),
        ("POST", "/admin/products/{id}/variants"),
        ("POST", "/admin/price-sets"),
        ("POST", "/admin/product-variants/{id}/price-set"),
        ("POST", "/admin/prices"),
        ("POST", "/admin/inventory-items"),
        ("POST", "/admin/inventory-items/{id}/location-levels"),
        ("GET", "/admin/order-baskets/{id}"),
        ("GET", "/admin/order-baskets/{id}/orders"),
        ("GET", "/admin/order-baskets/{id}/carts"),
        ("GET", "/admin/workflows-executions"),
        ("GET", "/admin/workflows-executions/{id}"),
        ("GET", "/admin/workflows-executions/{id}/steps"),
        ("GET", "/admin/workflow-dead-letters"),
        ("GET", "/admin/commission-rules"),
        ("GET", "/admin/orders/{id}/payout-lines"),
        ("GET", "/admin/payouts"),
        ("GET", "/admin/payout-balance/{currency_code}"),
        ("GET", "/admin/orders/{id}/fulfillments"),
        ("GET", "/admin/orders/{id}/shipping-options"),
        ("GET", "/admin/orders/{id}/returns/shipping-options"),
        ("GET", "/admin/orders/{id}/fulfillments/{fulfillment_id}"),
        ("GET", "/admin/fulfillment-sets"),
        ("GET", "/admin/fulfillment-sets/{id}/service-zones"),
        ("GET", "/admin/fulfillment-providers"),
        ("GET", "/admin/shipping-options"),
        ("GET", "/admin/shipping-options/{id}"),
        ("GET", "/admin/shipping-options/{id}/translations"),
        ("GET", "/admin/shipping-options/{id}/translations/{locale}"),
        ("GET", "/admin/shipping-profiles"),
        ("GET", "/admin/shipping-profiles/{id}"),
        ("GET", "/admin/shipping-option-types"),
        ("GET", "/admin/tax-regions"),
        ("GET", "/admin/tax-regions/{id}"),
        ("GET", "/admin/tax-rates"),
        ("GET", "/admin/tax-rates/{id}"),
        ("GET", "/admin/tax-rates/{id}/rules"),
        ("GET", "/admin/tax-registrations"),
        ("GET", "/admin/customers/{id}/tax-ids"),
        ("GET", "/admin/customers/{id}/tax-exemptions"),
        ("GET", "/admin/price-sets/{id}"),
        ("GET", "/admin/price-sets/{id}/prices"),
        ("GET", "/admin/product-variants/{id}/bundle/components"),
        ("GET", "/admin/product-variants/{id}/bundle/price"),
        ("GET", "/admin/prices/{id}/rules"),
        ("GET", "/admin/price-lists"),
        ("GET", "/admin/price-lists/{id}"),
        ("GET", "/admin/price-preferences"),
        ("GET", "/admin/payments"),
        ("GET", "/admin/payments/{id}"),
        ("GET", "/admin/payments/payment-providers"),
        ("GET", "/admin/payment-collections/{id}"),
        ("GET", "/admin/payment-collections/{id}/payment-sessions"),
        ("GET", "/admin/refund-reasons"),
        ("GET", "/admin/gift-cards"),
        ("GET", "/admin/gift-cards/{id}"),
        ("GET", "/admin/gift-cards/{id}/transactions"),
        ("GET", "/admin/customers/{id}/store-credit"),
        ("GET", "/admin/store-credits/{id}/transactions"),
        ("GET", "/admin/orders/{id}/entitlements"),
        ("POST", "/admin/orders/{id}/entitlements/revoke"),
        ("GET", "/admin/variants/{id}/digital-content"),
        ("POST", "/admin/variants/{id}/digital-content"),
        ("DELETE", "/admin/digital-content/{id}"),
        ("GET", "/admin/carts"),
    ];

    let router = Router::new()
        .route("/admin/products", get(list_products).post(create_product))
        .route("/admin/products/{id}", get(get_product))
        .route("/admin/orders", get(list_orders))
        .route("/admin/orders/{id}", get(get_order))
        .route(
            "/admin/inventory-items",
            get(list_inventory_items).post(create_inventory_item),
        )
        .route("/admin/inventory-items/{id}", get(get_inventory_item))
        .route("/admin/customers", get(list_customers))
        .route("/admin/customers/{id}", get(get_customer))
        .route("/admin/promotions", get(list_promotions))
        .route("/admin/promotions/{id}", get(get_promotion))
        .route("/admin/subscriptions", get(list_subscriptions))
        .route("/admin/subscriptions/{id}", get(get_subscription))
        .route("/admin/regions", get(list_regions).post(create_region))
        .route("/admin/regions/{id}", get(get_region))
        .route(
            "/admin/sales-channels",
            get(list_sales_channels).post(create_sales_channel),
        )
        .route("/admin/sales-channels/{id}", get(get_sales_channel))
        .route(
            "/admin/currencies",
            get(list_currencies).post(create_currency),
        )
        .route(
            "/admin/publishable-api-keys",
            get(list_publishable_keys).post(create_publishable_key),
        )
        .route(
            "/admin/stock-locations",
            get(list_stock_locations).post(create_stock_location),
        )
        .route("/admin/products/{id}/variants", post(create_variant))
        .route("/admin/price-sets", post(create_price_set))
        .route(
            "/admin/product-variants/{id}/price-set",
            post(link_variant_price_set),
        )
        .route("/admin/prices", post(add_price))
        .route(
            "/admin/inventory-items/{id}/location-levels",
            post(set_stock),
        )
        .route("/admin/order-baskets/{id}", get(get_basket))
        .route("/admin/order-baskets/{id}/orders", get(basket_orders))
        .route("/admin/order-baskets/{id}/carts", get(basket_carts))
        .route("/admin/workflows-executions", get(list_workflow_runs))
        .route("/admin/workflows-executions/{id}", get(get_workflow_run))
        .route(
            "/admin/workflows-executions/{id}/steps",
            get(list_workflow_run_steps),
        )
        .route(
            "/admin/workflow-dead-letters",
            get(list_workflow_dead_letters),
        )
        .route("/admin/commission-rules", get(commission_rules))
        .route("/admin/orders/{id}/payout-lines", get(order_payout_lines))
        .route("/admin/payouts", get(list_payouts))
        .route("/admin/payout-balance/{currency_code}", get(payout_balance))
        .route("/admin/orders/{id}/fulfillments", get(order_fulfillments))
        .route(
            "/admin/orders/{id}/shipping-options",
            get(order_shipping_options),
        )
        .route(
            "/admin/orders/{id}/returns/shipping-options",
            get(return_shipping_options),
        )
        .route(
            "/admin/orders/{id}/fulfillments/{fulfillment_id}",
            get(get_fulfillment),
        )
        .route("/admin/fulfillment-sets", get(list_fulfillment_sets))
        .route(
            "/admin/fulfillment-sets/{id}/service-zones",
            get(service_zones),
        )
        .route("/admin/fulfillment-providers", get(fulfillment_providers))
        .route("/admin/shipping-options", get(list_shipping_options))
        .route("/admin/shipping-options/{id}", get(get_shipping_option))
        .route(
            "/admin/shipping-options/{id}/translations",
            get(list_shipping_option_translations),
        )
        .route(
            "/admin/shipping-options/{id}/translations/{locale}",
            get(localised_shipping_option),
        )
        .route("/admin/shipping-profiles", get(list_shipping_profiles))
        .route("/admin/shipping-profiles/{id}", get(get_shipping_profile))
        .route(
            "/admin/shipping-option-types",
            get(list_shipping_option_types),
        )
        .route("/admin/tax-regions", get(list_tax_regions))
        .route("/admin/tax-regions/{id}", get(get_tax_region))
        .route("/admin/tax-rates", get(list_tax_rates))
        .route("/admin/tax-rates/{id}", get(get_tax_rate))
        .route("/admin/tax-rates/{id}/rules", get(list_tax_rate_rules))
        .route("/admin/tax-registrations", get(list_tax_registrations))
        .route("/admin/customers/{id}/tax-ids", get(list_customer_tax_ids))
        .route(
            "/admin/customers/{id}/tax-exemptions",
            get(list_customer_tax_exemptions),
        )
        .route("/admin/price-sets/{id}", get(get_price_set))
        .route("/admin/price-sets/{id}/prices", get(list_prices))
        .route(
            "/admin/product-variants/{id}/bundle/components",
            get(list_bundle_components),
        )
        .route(
            "/admin/product-variants/{id}/bundle/price",
            get(bundle_price),
        )
        .route("/admin/prices/{id}/rules", get(list_price_rules))
        .route("/admin/price-lists", get(list_price_lists))
        .route("/admin/price-lists/{id}", get(get_price_list))
        .route("/admin/price-preferences", get(get_price_preference))
        .route("/admin/payments", get(list_payments))
        .route("/admin/payments/{id}", get(get_payment))
        .route("/admin/payments/payment-providers", get(payment_providers))
        .route(
            "/admin/payment-collections/{id}",
            get(get_payment_collection),
        )
        .route(
            "/admin/payment-collections/{id}/payment-sessions",
            get(payment_sessions),
        )
        .route("/admin/refund-reasons", get(list_refund_reasons))
        .route("/admin/gift-cards", get(list_gift_cards))
        .route("/admin/gift-cards/{id}", get(get_gift_card))
        .route(
            "/admin/gift-cards/{id}/transactions",
            get(gift_card_movements),
        )
        .route("/admin/customers/{id}/store-credit", get(get_store_credit))
        .route(
            "/admin/store-credits/{id}/transactions",
            get(store_credit_movements),
        )
        .route(
            "/admin/orders/{id}/entitlements",
            get(list_order_entitlements),
        )
        .route(
            "/admin/orders/{id}/entitlements/revoke",
            post(revoke_entitlements),
        )
        .route(
            "/admin/variants/{id}/digital-content",
            get(list_content).post(put_content),
        )
        .route("/admin/digital-content/{id}", delete(delete_content))
        .route("/admin/carts", get(list_carts));

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

async fn get_product(
    State(state): State<AppState>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let product = admin_catalogue::get_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(product))
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

async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<OrderId>,
) -> Result<Json<admin_order::OrderView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let order = admin_order::get_order(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
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

async fn get_inventory_item(
    State(state): State<AppState>,
    Path(id): Path<InventoryItemId>,
) -> Result<Json<admin_catalogue::InventoryItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let item = admin_catalogue::get_inventory_item(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(item))
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

async fn get_customer(
    State(state): State<AppState>,
    Path(id): Path<CustomerId>,
) -> Result<Json<admin_rest::CustomerView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let customer = admin_rest::get_customer(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(customer))
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

async fn get_promotion(
    State(state): State<AppState>,
    Path(id): Path<PromotionId>,
) -> Result<Json<admin_rest::PromotionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let promotion = admin_rest::get_promotion(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(promotion))
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

async fn get_subscription(
    State(state): State<AppState>,
    Path(id): Path<SubscriptionId>,
) -> Result<Json<subscription::ContractView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let contract = subscription::get_subscription(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(contract))
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

async fn get_region(
    State(state): State<AppState>,
    Path(id): Path<RegionId>,
) -> Result<Json<admin_rest::RegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let region = admin_rest::get_region(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(region))
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

async fn get_sales_channel(
    State(state): State<AppState>,
    Path(id): Path<SalesChannelId>,
) -> Result<Json<admin_rest::SalesChannelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let channel = admin_rest::get_sales_channel(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(channel))
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

async fn create_currency(
    State(state): State<AppState>,
    Json(body): Json<admin_rest::CreateCurrency>,
) -> Result<Json<admin_rest::CurrencyView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let currency = admin_rest::create_currency(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(currency))
}

async fn create_region(
    State(state): State<AppState>,
    Json(body): Json<admin_rest::CreateRegion>,
) -> Result<Json<admin_rest::RegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let region = admin_rest::create_region(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn create_sales_channel(
    State(state): State<AppState>,
    Json(body): Json<admin_rest::CreateSalesChannel>,
) -> Result<Json<admin_rest::SalesChannelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let channel = admin_rest::create_sales_channel(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(channel))
}

async fn list_publishable_keys(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::PublishableKeyView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_publishable_keys(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_publishable_key(
    State(state): State<AppState>,
    Json(body): Json<admin_rest::CreatePublishableKey>,
) -> Result<Json<admin_rest::IssuedKeyView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let key = admin_rest::create_publishable_key(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(key))
}

async fn list_stock_locations(
    State(state): State<AppState>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::StockLocationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_catalogue::list_stock_locations(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_stock_location(
    State(state): State<AppState>,
    Json(body): Json<admin_catalogue::CreateStockLocation>,
) -> Result<Json<admin_catalogue::StockLocationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let location = admin_catalogue::create_stock_location(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(location))
}

async fn create_product(
    State(state): State<AppState>,
    Json(body): Json<admin_catalogue::CreateProduct>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let product = admin_catalogue::create_product(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(product))
}

async fn create_variant(
    State(state): State<AppState>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::CreateVariant>,
) -> Result<Json<admin_catalogue::VariantView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let variant = admin_catalogue::create_variant(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(variant))
}

async fn create_price_set(
    State(state): State<AppState>,
) -> Result<Json<admin_catalogue::PriceSetView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let set = admin_catalogue::create_price_set(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(set))
}

async fn link_variant_price_set(
    State(state): State<AppState>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::LinkPriceSet>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    admin_catalogue::link_variant_price_set(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_price(
    State(state): State<AppState>,
    Json(body): Json<admin_catalogue::AddPrice>,
) -> Result<Json<admin_catalogue::PriceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let price = admin_catalogue::add_price(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(price))
}

async fn create_inventory_item(
    State(state): State<AppState>,
    Json(body): Json<admin_catalogue::CreateInventoryItem>,
) -> Result<Json<admin_catalogue::InventoryItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let item = admin_catalogue::create_inventory_item(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(item))
}

async fn set_stock(
    State(state): State<AppState>,
    Path(id): Path<InventoryItemId>,
    Json(body): Json<admin_catalogue::SetStock>,
) -> Result<Json<admin_catalogue::InventoryLevelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let level = admin_catalogue::set_stock(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(level))
}

async fn get_basket(
    State(state): State<AppState>,
    Path(id): Path<OrderBasketId>,
) -> Result<Json<order_basket::BasketView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let basket = order_basket::get_basket(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(basket))
}

async fn basket_orders(
    State(state): State<AppState>,
    Path(id): Path<OrderBasketId>,
    Query(query): Query<order_basket::ListBasketOrders>,
) -> Result<Json<tezgah::page::Page<admin_order::OrderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = order_basket::basket_orders(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn basket_carts(
    State(state): State<AppState>,
    Path(id): Path<OrderBasketId>,
    Query(query): Query<order_basket::ListBasketOrders>,
) -> Result<Json<tezgah::page::Page<store_api::CartView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = order_basket::basket_carts(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_workflow_runs(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::ListWorkflowRuns>,
) -> Result<Json<tezgah::page::Page<admin_rest::WorkflowRunSummaryView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_workflow_runs(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_workflow_run(
    State(state): State<AppState>,
    Path(id): Path<WorkflowRunId>,
) -> Result<Json<admin_rest::WorkflowRunView>, ApiError> {
    let ctx = ctx_for(&state);
    let run = admin_rest::get_workflow_run(&state.pool, &ctx, id).await?;
    Ok(Json(run))
}

async fn list_workflow_run_steps(
    State(state): State<AppState>,
    Path(id): Path<WorkflowRunId>,
) -> Result<Json<Vec<admin_rest::WorkflowStepView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let steps = admin_rest::list_workflow_run_steps(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(steps))
}

async fn list_workflow_dead_letters(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::WorkflowDeadLetterView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_workflow_dead_letters(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn commission_rules(
    State(state): State<AppState>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::CommissionRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = payout::commission_rules(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn order_payout_lines(
    State(state): State<AppState>,
    Path(id): Path<OrderId>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::PayoutLineView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = payout::order_payout_lines(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_payouts(
    State(state): State<AppState>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::PayoutView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = payout::payouts(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn payout_balance(
    State(state): State<AppState>,
    Path(currency_code): Path<String>,
) -> Result<Json<payout::BalanceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let balance = payout::balance(&mut tx, &ctx, currency_code).await?;
    tx.commit().await?;
    Ok(Json(balance))
}

#[derive(serde::Deserialize)]
struct CountryQuery {
    country_code: String,
}

async fn order_fulfillments(
    State(state): State<AppState>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::FulfillmentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::order_fulfillments(&mut tx, &ctx, order_id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn order_shipping_options(
    State(state): State<AppState>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<CountryQuery>,
) -> Result<Json<Vec<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let options =
        admin_order::order_shipping_options(&mut tx, &ctx, order_id, &query.country_code).await?;
    tx.commit().await?;
    Ok(Json(options))
}

async fn return_shipping_options(
    State(state): State<AppState>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<CountryQuery>,
) -> Result<Json<Vec<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let options =
        admin_order::return_shipping_options(&mut tx, &ctx, order_id, &query.country_code).await?;
    tx.commit().await?;
    Ok(Json(options))
}

async fn get_fulfillment(
    State(state): State<AppState>,
    Path((order_id, id)): Path<(OrderId, FulfillmentId)>,
) -> Result<Json<admin_order::FulfillmentDetailView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let fulfillment = admin_order::get_fulfillment(&mut tx, &ctx, order_id, id).await?;
    tx.commit().await?;
    Ok(Json(fulfillment))
}

async fn list_fulfillment_sets(
    State(state): State<AppState>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::FulfillmentSetView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_fulfillment_sets(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn service_zones(
    State(state): State<AppState>,
    Path(id): Path<FulfillmentSetId>,
) -> Result<Json<Vec<admin_order::ServiceZoneView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let zones = admin_order::service_zones(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(zones))
}

async fn fulfillment_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<admin_order::ProviderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let providers = admin_order::fulfillment_providers(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(providers))
}

async fn list_shipping_options(
    State(state): State<AppState>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_shipping_options(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_shipping_option(
    State(state): State<AppState>,
    Path(id): Path<ShippingOptionId>,
) -> Result<Json<admin_order::ShippingOptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let option = admin_order::get_shipping_option(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(option))
}

async fn list_shipping_option_translations(
    State(state): State<AppState>,
    Path(id): Path<ShippingOptionId>,
) -> Result<Json<Vec<admin_order::ShippingOptionTranslationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let translations = admin_order::list_shipping_option_translations(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(translations))
}

async fn localised_shipping_option(
    State(state): State<AppState>,
    Path((id, locale)): Path<(ShippingOptionId, String)>,
) -> Result<Json<admin_order::LocalisedShippingOptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let translation = admin_order::localised_shipping_option(&mut tx, &ctx, id, &locale).await?;
    tx.commit().await?;
    Ok(Json(translation))
}

async fn list_shipping_profiles(
    State(state): State<AppState>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingProfileView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_shipping_profiles(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_shipping_profile(
    State(state): State<AppState>,
    Path(id): Path<ShippingProfileId>,
) -> Result<Json<admin_order::ShippingProfileView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let profile = admin_order::get_shipping_profile(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(profile))
}

async fn list_shipping_option_types(
    State(state): State<AppState>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingOptionTypeView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_shipping_option_types(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_tax_regions(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::TaxRegionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_tax_regions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_tax_region(
    State(state): State<AppState>,
    Path(id): Path<TaxRegionId>,
) -> Result<Json<admin_rest::TaxRegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let region = admin_rest::get_tax_region(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn list_tax_rates(
    State(state): State<AppState>,
    Query(query): Query<admin_rest::ListTaxRates>,
) -> Result<Json<tezgah::page::Page<admin_rest::TaxRateView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_rest::list_tax_rates(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_tax_rate(
    State(state): State<AppState>,
    Path(id): Path<TaxRateId>,
) -> Result<Json<admin_rest::TaxRateView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let rate = admin_rest::get_tax_rate(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rate))
}

async fn list_tax_rate_rules(
    State(state): State<AppState>,
    Path(id): Path<TaxRateId>,
) -> Result<Json<Vec<admin_rest::TaxRateRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let rules = admin_rest::list_tax_rate_rules(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rules))
}

async fn list_tax_registrations(
    State(state): State<AppState>,
) -> Result<Json<Vec<tax_identity::RegistrationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let registrations = tax_identity::list_registrations(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(registrations))
}

async fn list_customer_tax_ids(
    State(state): State<AppState>,
    Path(id): Path<CustomerId>,
) -> Result<Json<Vec<tax_identity::TaxIdView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let ids = tax_identity::list_tax_ids(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(ids))
}

async fn list_customer_tax_exemptions(
    State(state): State<AppState>,
    Path(id): Path<CustomerId>,
) -> Result<Json<Vec<tax_identity::ExemptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let exemptions = tax_identity::list_exemptions(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(exemptions))
}

async fn get_price_set(
    State(state): State<AppState>,
    Path(id): Path<PriceSetId>,
) -> Result<Json<admin_catalogue::PriceSetView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let set = admin_catalogue::get_price_set(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(set))
}

async fn list_prices(
    State(state): State<AppState>,
    Path(id): Path<PriceSetId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::PriceView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_catalogue::list_prices(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_bundle_components(
    State(state): State<AppState>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<admin_catalogue::BundleComponentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let components = admin_catalogue::list_bundle_components(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(components))
}

async fn bundle_price(
    State(state): State<AppState>,
    Path(id): Path<VariantId>,
    Query(query): Query<admin_catalogue::BundlePriceQuery>,
) -> Result<Json<admin_catalogue::BundlePriceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let price = admin_catalogue::bundle_price(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(price))
}

async fn list_price_rules(
    State(state): State<AppState>,
    Path(id): Path<PriceId>,
) -> Result<Json<Vec<admin_catalogue::PriceRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let rules = admin_catalogue::list_price_rules(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rules))
}

async fn list_price_lists(
    State(state): State<AppState>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::PriceListView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_catalogue::list_price_lists(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_price_list(
    State(state): State<AppState>,
    Path(id): Path<PriceListId>,
) -> Result<Json<admin_catalogue::PriceListView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let list = admin_catalogue::get_price_list(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(list))
}

async fn get_price_preference(
    State(state): State<AppState>,
    Query(query): Query<admin_catalogue::FindPricePreference>,
) -> Result<Json<Option<admin_catalogue::PricePreferenceView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let preference = admin_catalogue::get_price_preference(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(preference))
}

async fn list_payments(
    State(state): State<AppState>,
    Query(query): Query<admin_order::ListPayments>,
) -> Result<Json<tezgah::page::Page<admin_order::PaymentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_payments(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_payment(
    State(state): State<AppState>,
    Path(id): Path<PaymentId>,
) -> Result<Json<admin_order::PaymentView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let payment = admin_order::get_payment(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(payment))
}

async fn payment_providers(
    State(state): State<AppState>,
) -> Result<Json<Vec<admin_order::ProviderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let providers = admin_order::payment_providers(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(providers))
}

async fn get_payment_collection(
    State(state): State<AppState>,
    Path(id): Path<PaymentCollectionId>,
) -> Result<Json<admin_order::CollectionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let collection = admin_order::get_payment_collection(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(collection))
}

async fn payment_sessions(
    State(state): State<AppState>,
    Path(id): Path<PaymentCollectionId>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::SessionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::payment_sessions(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_refund_reasons(
    State(state): State<AppState>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ReasonView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = admin_order::list_refund_reasons(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_gift_cards(
    State(state): State<AppState>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::GiftCardView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = credit::list_gift_cards(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_gift_card(
    State(state): State<AppState>,
    Path(id): Path<GiftCardId>,
) -> Result<Json<credit::GiftCardView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let card = credit::get_gift_card(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(card))
}

async fn gift_card_movements(
    State(state): State<AppState>,
    Path(id): Path<GiftCardId>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::CreditMovementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = credit::gift_card_movements(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_store_credit(
    State(state): State<AppState>,
    Path(id): Path<CustomerId>,
    Query(query): Query<credit::BalanceQuery>,
) -> Result<Json<credit::StoreCreditView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let balance = credit::get_store_credit(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(balance))
}

async fn store_credit_movements(
    State(state): State<AppState>,
    Path(id): Path<StoreCreditId>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::CreditMovementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = credit::store_credit_movements(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_order_entitlements(
    State(state): State<AppState>,
    Path(id): Path<OrderId>,
) -> Result<Json<Vec<digital::EntitlementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let entitlements = digital::list_order_entitlements(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(entitlements))
}

async fn revoke_entitlements(
    State(state): State<AppState>,
    Path(id): Path<OrderId>,
    Json(body): Json<digital::RevokeEntitlements>,
) -> Result<Json<Vec<digital::EntitlementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let entitlements = digital::revoke_entitlements(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(entitlements))
}

async fn list_content(
    State(state): State<AppState>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<digital::ContentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let content = digital::list_content(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(content))
}

async fn put_content(
    State(state): State<AppState>,
    Path(id): Path<VariantId>,
    Json(body): Json<digital::PutContent>,
) -> Result<Json<digital::ContentView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let content = digital::put_content(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(content))
}

async fn delete_content(
    State(state): State<AppState>,
    Path(id): Path<DigitalContentId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    digital::delete_content(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_carts(
    State(state): State<AppState>,
    Query(query): Query<order_basket::ListCarts>,
) -> Result<Json<tezgah::page::Page<store_api::CartView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    let page = order_basket::list_carts(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}
