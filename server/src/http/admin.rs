//! The admin surface: one list endpoint per screen the panel in `client/`
//! draws — products, orders, inventory items, customers, promotions,
//! subscriptions, and the two the store screen's tabs read, regions and
//! sales channels — plus the currencies list the overview screen reads, and
//! the two lists the panel needs and did not have: publishable API keys and
//! stock locations. Alongside every list, the one-row read behind it: a
//! click on a row in any of those seven screens has somewhere to go —
//! `GET /admin/{products,orders,inventory-items,customers,promotions,
//! regions,sales-channels,subscriptions}/{id}`. Alongside them, the writes a
//! fresh install needs to reach its first order and cannot make any other
//! way: enabling a currency, opening a region, a sales channel and a stock
//! location, minting a publishable key, and creating a product, its
//! variants, a price and a stocked inventory level — #214. Past that, the
//! list and single-row read behind an order basket, a workflow run and a
//! seller scope's own payouts: `GET /admin/order-baskets/{id}` and its two
//! sub-lists, `GET /admin/workflows-executions` and the single run, its
//! steps and the scope-wide dead letters, and `GET /admin/commission-rules`,
//! `/admin/orders/{id}/payout-lines`, `/admin/payouts` and
//! `/admin/payout-balance/{currency_code}` — none of the three has a screen
//! in `client/` yet, so nothing calls them but this binary's own startup
//! count. Past all of that, deleting a row a screen already lists: `DELETE
//! /admin/inventory-items/{id}` (`admin_catalogue::delete_inventory_item`,
//! `Action::Delete`). `tezgah::api` has no update for an inventory item
//! itself — only for the stock a location holds of it, at
//! `POST /admin/inventory-items/{id}/location-levels`, already bound above
//! — so there is no `PATCH /admin/inventory-items/{id}` here to bind.
//! Everything else `tezgah::api` offers stays unbound; nothing here was
//! chosen for this binary beyond what those needs cover.
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
use axum::routing::{get, post};
use axum::{Json, Router};
use subtle::ConstantTimeEq;
use tezgah::api::{
    admin_catalogue, admin_order, admin_rest, order_basket, payout, store as store_api,
    subscription,
};
use tezgah::id::{
    CustomerId, InventoryItemId, OrderBasketId, OrderId, ProductId, PromotionId, RegionId,
    SalesChannelId, SubscriptionId, VariantId, WorkflowRunId,
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
        ("DELETE", "/admin/inventory-items/{id}"),
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
        .route(
            "/admin/inventory-items/{id}",
            get(get_inventory_item).delete(delete_inventory_item),
        )
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
        .route("/admin/payout-balance/{currency_code}", get(payout_balance));

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

async fn delete_inventory_item(
    State(state): State<AppState>,
    Path(id): Path<InventoryItemId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state);
    admin_catalogue::delete_inventory_item(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
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
