//! draws, the single-row read behind a click on any of them, the twelve
//! writes a fresh install needs to reach its first order (#214), editing
//! and deleting a row a screen already lists wherever `tezgah::api` has the
//! function for it (`PATCH` on products, regions, sales channels,
//! promotions, customers and stock locations; `DELETE` on all of those but
//! regions, plus inventory items — `../README.md`'s route table names each
//! one), and, past the panel, every list-and-single-read a domain already
//! had the functions for in `src/api/` with nothing here calling them —
//! order-basket, workflow, payout, fulfilment, tax, pricing, payment,
//! credit and (list, single read and writes both, because the domain had
//! no route at all) digital. `../README.md`'s own route table carries the
//! full breakdown, kept there rather than duplicated here because it moves
//! with every domain this binary picks up next. Everything else
//! `tezgah::api` offers stays unbound; nothing here was chosen for this
//! binary beyond what those needs cover.
//!
//! # Two credentials, one door
//!
//! `docs/hosting.md` and `tezgah::ports::Authorizer` are explicit that tezgah
//! authenticates nobody — a host supplies its own roles, or, as `ServerHost`
//! does, supplies none and grants every actor. That is right for a library
//! and leaves the product a hole, which this module used to be the whole of:
//! one shared `ADMIN_TOKEN`, so a shop with two employees had one credential
//! between them, nothing to revoke when one left, and every audit row naming
//! the same nil uuid.
//!
//! `crate::identity` is the other half now. A session token belongs to a
//! person, expires, and dies when that person is disabled or changes their
//! password. `ADMIN_TOKEN` stays, because something has to make the first
//! account and something has to get back in when the last password is lost —
//! and it is what it always was: a shared secret, not a person, which is why
//! `Caller::actor_id` still hands tezgah the nil uuid for it rather than
//! inventing an identity.
//!
//! What has not changed: this is still authentication, not authorization.
//! Whoever clears the gate reaches `ctx_for` as `Actor::Staff`, and
//! `ServerHost::authorize` grants every `tezgah::ports::Action` to it — `View`
//! and `Write` and `Delete` alike. `Authorizer::authorize` already receives
//! the `Action` on every call, and a `Caller` is now on the request beside it,
//! so a role carried on an operator row is the seam a split would use.
//! Nothing here answers that yet; #214 raises it.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use crate::http::auth::Caller;
use crate::identity::Role;

use axum::extract::{MatchedPath, Path, Query, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::{Extension, Json, Router};
use tezgah::api::{
    admin_catalogue, admin_order, admin_rest, agreement, credit, digital, inventory_lot,
    order_basket, payout, store as store_api, subscription, tax_identity,
};
use tezgah::id::{
    CampaignId, CategoryId, ClaimId, CustomerGroupId, CustomerId, DigitalContentId, ExchangeId,
    FulfillmentId, FulfillmentSetId, GiftCardId, InventoryItemId, InventoryLotId, OptionId,
    OrderBasketId, OrderChangeId, OrderId, PaymentCollectionId, PaymentId, PaymentWebhookEventId,
    PriceId, PriceListId, PriceSetId, ProductId, ProductTagId, PromotionId, PublishableKeyId,
    RegionId, ReservationId, ReturnId, SalesChannelId, SellingPlanGroupId, SellingPlanId,
    ShippingOptionId, ShippingProfileId, StockLocationId, StoreCreditId, SubscriptionId, TaxRateId,
    TaxRegionId, VariantId, WorkflowRunId,
};
use tezgah::ports::{Action, Actor, Ctx, Host};

use super::{ApiError, AppState, begin};

pub fn router() -> (Router<AppState>, Vec<(&'static str, &'static str)>) {
    let bound = vec![
        ("GET", "/admin/products"),
        ("GET", "/admin/products/{id}"),
        ("PATCH", "/admin/products/{id}"),
        ("DELETE", "/admin/products/{id}"),
        ("GET", "/admin/orders"),
        ("GET", "/admin/orders/{id}"),
        ("GET", "/admin/inventory-items"),
        ("GET", "/admin/inventory-items/{id}"),
        ("DELETE", "/admin/inventory-items/{id}"),
        ("GET", "/admin/products/export"),
        ("POST", "/admin/products/batch"),
        ("POST", "/admin/prices/batch"),
        ("POST", "/admin/inventory-items/batch"),
        ("GET", "/admin/customers"),
        ("GET", "/admin/customers/{id}"),
        ("PATCH", "/admin/customers/{id}"),
        ("DELETE", "/admin/customers/{id}"),
        ("GET", "/admin/promotions"),
        ("GET", "/admin/promotions/{id}"),
        ("PATCH", "/admin/promotions/{id}"),
        ("DELETE", "/admin/promotions/{id}"),
        ("GET", "/admin/subscriptions"),
        ("GET", "/admin/orders/{id}/invoices"),
        ("POST", "/admin/orders/{id}/invoices"),
        ("GET", "/admin/orders/{id}/agreements"),
        ("GET", "/admin/orders/{id}/withdrawal"),
        ("POST", "/admin/promotions/{id}/status"),
        ("POST", "/admin/promotions/{id}/application-method"),
        ("GET", "/admin/customers/{id}/addresses"),
        ("POST", "/admin/customers/{id}/addresses"),
        ("POST", "/admin/customers/{id}/erase"),
        ("POST", "/admin/gift-cards/{id}/adjust"),
        ("POST", "/admin/gift-cards/{id}/disable"),
        ("POST", "/admin/publishable-api-keys/{id}/revoke"),
        ("GET", "/admin/regions/{id}/countries"),
        ("POST", "/admin/regions/{id}/countries"),
        ("GET", "/admin/stock-locations/{id}/address"),
        ("POST", "/admin/stock-locations/{id}/address"),
        ("POST", "/admin/tax-exemptions/{id}/revoke"),
        ("GET", "/admin/subscriptions/{id}"),
        ("GET", "/admin/subscriptions/{id}/events"),
        ("POST", "/admin/subscriptions/{id}/cancel"),
        ("POST", "/admin/subscriptions/{id}/pause"),
        ("POST", "/admin/subscriptions/{id}/resume"),
        ("POST", "/admin/subscriptions/{id}/skip"),
        ("POST", "/admin/subscriptions/{id}/swap"),
        ("POST", "/admin/subscriptions/{id}/deliver"),
        ("GET", "/admin/regions"),
        ("GET", "/admin/regions/{id}"),
        ("PATCH", "/admin/regions/{id}"),
        ("GET", "/admin/sales-channels"),
        ("GET", "/admin/sales-channels/{id}"),
        ("PATCH", "/admin/sales-channels/{id}"),
        ("DELETE", "/admin/sales-channels/{id}"),
        ("GET", "/admin/currencies"),
        ("GET", "/admin/publishable-api-keys"),
        ("GET", "/admin/stock-locations"),
        ("PATCH", "/admin/stock-locations/{id}"),
        ("DELETE", "/admin/stock-locations/{id}"),
        ("POST", "/admin/currencies"),
        ("POST", "/admin/regions"),
        ("POST", "/admin/sales-channels"),
        ("POST", "/admin/publishable-api-keys"),
        ("POST", "/admin/stock-locations"),
        ("POST", "/admin/products"),
        ("POST", "/admin/products/{id}/publish"),
        ("POST", "/admin/products/{id}/archive"),
        ("POST", "/admin/products/{id}/submit"),
        ("POST", "/admin/products/{id}/approve"),
        ("POST", "/admin/products/{id}/reject"),
        ("GET", "/admin/products/{id}/tags"),
        ("POST", "/admin/products/{id}/tags"),
        ("DELETE", "/admin/products/{id}/tags/{tag_id}"),
        ("GET", "/admin/products/{id}/categories"),
        ("POST", "/admin/products/{id}/categories"),
        ("DELETE", "/admin/products/{id}/categories/{category_id}"),
        ("GET", "/admin/products/{id}/channels"),
        ("POST", "/admin/products/{id}/channels"),
        ("DELETE", "/admin/products/{id}/channels/{sales_channel_id}"),
        ("POST", "/admin/campaigns/{id}/budget"),
        ("POST", "/admin/campaigns/{id}/promotions"),
        ("GET", "/admin/customer-groups/{id}/customers"),
        ("POST", "/admin/customer-groups/{id}/customers"),
        ("GET", "/admin/customers/{id}/export"),
        ("POST", "/admin/inventory-items/{id}/transfers"),
        ("GET", "/admin/inventory-items/{id}/transfers"),
        ("POST", "/admin/price-lists/{id}/rules"),
        ("GET", "/admin/products/{id}/translations"),
        ("POST", "/admin/products/{id}/translations"),
        ("GET", "/admin/product-categories/{id}/translations"),
        ("POST", "/admin/product-categories/{id}/translations"),
        ("POST", "/admin/product-variants/{id}/bundle"),
        ("GET", "/admin/product-variants/{id}/inventory-items"),
        ("POST", "/admin/product-variants/{id}/inventory-items"),
        ("GET", "/admin/publishable-api-keys/{id}/sales-channels"),
        ("POST", "/admin/publishable-api-keys/{id}/sales-channels"),
        ("POST", "/admin/reservations/{id}/fulfil"),
        ("GET", "/admin/exchanges"),
        ("POST", "/admin/exchanges"),
        ("GET", "/admin/exchanges/{id}"),
        ("GET", "/admin/exchanges/{id}/items"),
        ("POST", "/admin/exchanges/{id}/cancel"),
        ("POST", "/admin/exchanges/{id}/request"),
        ("POST", "/admin/exchanges/{id}/inbound/items"),
        ("DELETE", "/admin/exchanges/{id}/inbound/items/{action_id}"),
        ("POST", "/admin/exchanges/{id}/inbound/shipping-method"),
        ("POST", "/admin/exchanges/{id}/outbound/items"),
        ("DELETE", "/admin/exchanges/{id}/outbound/items/{action_id}"),
        ("POST", "/admin/exchanges/{id}/outbound/shipping-method"),
        ("GET", "/admin/order-edits/{id}"),
        ("DELETE", "/admin/order-edits/{id}"),
        ("POST", "/admin/order-edits/{id}/confirm"),
        ("POST", "/admin/order-edits/{id}/items"),
        ("DELETE", "/admin/order-edits/{id}/items/{action_id}"),
        ("POST", "/admin/order-edits/{id}/shipping-method"),
        (
            "DELETE",
            "/admin/order-edits/{id}/shipping-method/{action_id}",
        ),
        ("GET", "/admin/claims"),
        ("POST", "/admin/claims"),
        ("GET", "/admin/claims/{id}"),
        ("GET", "/admin/claims/{id}/items"),
        ("GET", "/admin/claims/{id}/lines"),
        ("POST", "/admin/claims/{id}/cancel"),
        ("POST", "/admin/claims/{id}/request"),
        ("POST", "/admin/claims/{id}/claim-items"),
        ("DELETE", "/admin/claims/{id}/claim-items/{action_id}"),
        ("POST", "/admin/claims/{id}/inbound/items"),
        ("DELETE", "/admin/claims/{id}/inbound/items/{action_id}"),
        ("POST", "/admin/claims/{id}/inbound/shipping-method"),
        ("POST", "/admin/claims/{id}/outbound/items"),
        ("DELETE", "/admin/claims/{id}/outbound/items/{action_id}"),
        ("POST", "/admin/claims/{id}/outbound/shipping-method"),
        ("GET", "/admin/returns"),
        ("POST", "/admin/returns"),
        ("GET", "/admin/returns/{id}"),
        ("GET", "/admin/returns/{id}/items"),
        ("POST", "/admin/returns/{id}/receive"),
        ("POST", "/admin/returns/{id}/dismiss-items"),
        ("POST", "/admin/returns/{id}/cancel"),
        ("POST", "/admin/returns/{id}/request"),
        ("POST", "/admin/returns/{id}/request-items"),
        ("DELETE", "/admin/returns/{id}/request-items/{action_id}"),
        ("POST", "/admin/returns/{id}/receive-items"),
        ("DELETE", "/admin/returns/{id}/receive-items/{action_id}"),
        ("POST", "/admin/returns/{id}/shipping-method"),
        ("DELETE", "/admin/returns/{id}/shipping-method/{action_id}"),
        ("GET", "/admin/return-reasons"),
        ("POST", "/admin/return-reasons"),
        ("GET", "/admin/return-reasons/{id}/translations"),
        ("POST", "/admin/return-reasons/{id}/translations"),
        ("GET", "/admin/return-reasons/{id}/translations/{locale}"),
        ("DELETE", "/admin/return-reasons/{id}/translations/{locale}"),
        ("POST", "/admin/returns/{id}/withdrawal"),
        ("POST", "/admin/selling-plan-groups/{id}/plans"),
        ("GET", "/admin/selling-plan-groups/{id}/plans"),
        ("POST", "/admin/selling-plans/{id}/variants"),
        ("GET", "/admin/products/{id}/images"),
        ("POST", "/admin/products/{id}/images"),
        ("GET", "/admin/products/{id}/options"),
        ("POST", "/admin/products/{id}/options"),
        ("GET", "/admin/product-variants/{id}/images"),
        ("POST", "/admin/product-variants/{id}/images"),
        ("GET", "/admin/product-variants/{id}/options"),
        ("POST", "/admin/product-variants/{id}/options"),
        ("POST", "/admin/product-options/{id}/values"),
        ("GET", "/admin/product-categories/{id}/subtree"),
        ("POST", "/admin/product-categories/{id}/move"),
        ("PATCH", "/admin/inventory-items/{id}/tracking"),
        ("GET", "/admin/inventory-items/{id}/lots"),
        ("POST", "/admin/inventory-items/{id}/lots"),
        ("GET", "/admin/inventory-lots/{id}/orders"),
        ("POST", "/admin/inventory-lots/{id}/reservations"),
        ("GET", "/admin/products/{id}/variants"),
        ("POST", "/admin/products/{id}/variants"),
        ("POST", "/admin/price-sets"),
        ("POST", "/admin/product-variants/{id}/price-set"),
        ("POST", "/admin/prices"),
        ("POST", "/admin/inventory-items"),
        ("GET", "/admin/inventory-items/{id}/location-levels"),
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
        ("GET", "/admin/payment-webhooks"),
        ("POST", "/admin/payment-webhooks/{id}/apply"),
        ("POST", "/admin/payment-webhooks/{id}/processed"),
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
        .route(
            "/admin/products/{id}",
            get(get_product)
                .patch(update_product)
                .delete(delete_product),
        )
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
        .route("/admin/products/export", get(export_products))
        .route("/admin/products/batch", post(batch_products))
        .route("/admin/prices/batch", post(batch_prices))
        .route("/admin/inventory-items/batch", post(batch_stock_levels))
        .route("/admin/customers", get(list_customers))
        .route(
            "/admin/customers/{id}",
            get(get_customer)
                .patch(update_customer)
                .delete(delete_customer),
        )
        .route("/admin/promotions", get(list_promotions))
        .route(
            "/admin/promotions/{id}",
            get(get_promotion)
                .patch(update_promotion)
                .delete(delete_promotion),
        )
        .route("/admin/subscriptions", get(list_subscriptions))
        .route(
            "/admin/orders/{id}/invoices",
            get(list_invoices).post(record_invoice),
        )
        .route("/admin/orders/{id}/agreements", get(order_agreements))
        .route("/admin/orders/{id}/withdrawal", get(order_withdrawal))
        .route("/admin/promotions/{id}/status", post(set_promotion_status))
        .route(
            "/admin/promotions/{id}/application-method",
            post(set_application_method),
        )
        .route(
            "/admin/customers/{id}/addresses",
            get(list_addresses).post(add_address),
        )
        .route("/admin/customers/{id}/erase", post(erase_customer))
        .route("/admin/gift-cards/{id}/adjust", post(adjust_gift_card))
        .route("/admin/gift-cards/{id}/disable", post(disable_gift_card))
        .route(
            "/admin/publishable-api-keys/{id}/revoke",
            post(revoke_publishable_key),
        )
        .route(
            "/admin/regions/{id}/countries",
            get(list_region_countries).post(add_region_country),
        )
        .route(
            "/admin/stock-locations/{id}/address",
            get(get_location_address).post(set_location_address),
        )
        .route("/admin/tax-exemptions/{id}/revoke", post(revoke_exemption))
        .route("/admin/subscriptions/{id}", get(get_subscription))
        .route("/admin/subscriptions/{id}/events", get(subscription_events))
        .route(
            "/admin/subscriptions/{id}/cancel",
            post(cancel_subscription),
        )
        .route("/admin/subscriptions/{id}/pause", post(pause_subscription))
        .route(
            "/admin/subscriptions/{id}/resume",
            post(resume_subscription),
        )
        .route("/admin/subscriptions/{id}/skip", post(skip_subscription))
        .route("/admin/subscriptions/{id}/swap", post(swap_subscription))
        .route(
            "/admin/subscriptions/{id}/deliver",
            post(deliver_subscription),
        )
        .route("/admin/regions", get(list_regions).post(create_region))
        .route("/admin/regions/{id}", get(get_region).patch(update_region))
        .route(
            "/admin/sales-channels",
            get(list_sales_channels).post(create_sales_channel),
        )
        .route(
            "/admin/sales-channels/{id}",
            get(get_sales_channel)
                .patch(update_sales_channel)
                .delete(delete_sales_channel),
        )
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
        .route(
            "/admin/stock-locations/{id}",
            patch(update_stock_location).delete(delete_stock_location),
        )
        .route("/admin/products/{id}/publish", post(publish_product))
        .route("/admin/products/{id}/archive", post(archive_product))
        .route("/admin/products/{id}/submit", post(submit_product))
        .route("/admin/products/{id}/approve", post(approve_product))
        .route("/admin/products/{id}/reject", post(reject_product))
        .route(
            "/admin/products/{id}/tags",
            get(list_product_tags).post(tag_product),
        )
        .route("/admin/products/{id}/tags/{tag_id}", delete(untag_product))
        .route(
            "/admin/products/{id}/categories",
            get(list_product_categories).post(add_product_to_category),
        )
        .route(
            "/admin/products/{id}/categories/{category_id}",
            delete(remove_product_from_category),
        )
        .route(
            "/admin/products/{id}/channels",
            get(list_product_channels).post(add_product_to_channel),
        )
        .route(
            "/admin/products/{id}/channels/{sales_channel_id}",
            delete(remove_product_from_channel),
        )
        .route("/admin/campaigns/{id}/budget", post(set_campaign_budget))
        .route(
            "/admin/campaigns/{id}/promotions",
            post(add_campaign_promotion),
        )
        .route(
            "/admin/customer-groups/{id}/customers",
            get(list_group_members).post(add_group_member),
        )
        .route("/admin/customers/{id}/export", get(export_customer))
        .route(
            "/admin/inventory-items/{id}/transfers",
            get(list_stock_transfers).post(transfer_stock),
        )
        .route("/admin/price-lists/{id}/rules", post(add_price_list_rule))
        .route(
            "/admin/products/{id}/translations",
            get(list_translations).post(put_translation),
        )
        .route(
            "/admin/product-categories/{id}/translations",
            get(list_category_translations).post(put_category_translation),
        )
        .route(
            "/admin/product-variants/{id}/bundle",
            post(set_bundle_price),
        )
        .route(
            "/admin/product-variants/{id}/inventory-items",
            get(list_variant_inventory_items).post(attach_inventory_item),
        )
        .route(
            "/admin/publishable-api-keys/{id}/sales-channels",
            get(list_key_sales_channels).post(link_key_sales_channel),
        )
        .route("/admin/reservations/{id}/fulfil", post(fulfil_reservation))
        .route(
            "/admin/exchanges",
            get(list_exchanges).post(request_exchange),
        )
        .route("/admin/exchanges/{id}", get(get_exchange))
        .route("/admin/exchanges/{id}/items", get(exchange_actions))
        .route("/admin/exchanges/{id}/cancel", post(cancel_exchange))
        .route(
            "/admin/exchanges/{id}/request",
            post(confirm_exchange_request),
        )
        .route(
            "/admin/exchanges/{id}/inbound/items",
            post(add_exchange_inbound_item),
        )
        .route(
            "/admin/exchanges/{id}/inbound/items/{action_id}",
            delete(remove_exchange_inbound_item),
        )
        .route(
            "/admin/exchanges/{id}/inbound/shipping-method",
            post(add_exchange_inbound_shipping),
        )
        .route(
            "/admin/exchanges/{id}/outbound/items",
            post(add_exchange_outbound_item),
        )
        .route(
            "/admin/exchanges/{id}/outbound/items/{action_id}",
            delete(remove_exchange_outbound_item),
        )
        .route(
            "/admin/exchanges/{id}/outbound/shipping-method",
            post(add_exchange_outbound_shipping),
        )
        .route(
            "/admin/order-edits/{id}",
            get(get_order_edit).delete(decline_order_edit),
        )
        .route("/admin/order-edits/{id}/confirm", post(confirm_order_edit))
        .route("/admin/order-edits/{id}/items", post(add_order_edit_item))
        .route(
            "/admin/order-edits/{id}/items/{action_id}",
            delete(remove_order_edit_item),
        )
        .route(
            "/admin/order-edits/{id}/shipping-method",
            post(add_order_edit_shipping),
        )
        .route(
            "/admin/order-edits/{id}/shipping-method/{action_id}",
            delete(remove_order_edit_shipping),
        )
        .route("/admin/claims", get(list_claims).post(request_claim))
        .route("/admin/claims/{id}", get(get_claim))
        .route("/admin/claims/{id}/items", get(claim_actions))
        .route("/admin/claims/{id}/lines", get(claim_lines))
        .route("/admin/claims/{id}/cancel", post(cancel_claim))
        .route("/admin/claims/{id}/request", post(confirm_claim_request))
        .route("/admin/claims/{id}/claim-items", post(add_claim_item))
        .route(
            "/admin/claims/{id}/claim-items/{action_id}",
            delete(remove_claim_item),
        )
        .route(
            "/admin/claims/{id}/inbound/items",
            post(add_claim_inbound_item),
        )
        .route(
            "/admin/claims/{id}/inbound/items/{action_id}",
            delete(remove_claim_inbound_item),
        )
        .route(
            "/admin/claims/{id}/inbound/shipping-method",
            post(add_claim_inbound_shipping),
        )
        .route(
            "/admin/claims/{id}/outbound/items",
            post(add_claim_outbound_item),
        )
        .route(
            "/admin/claims/{id}/outbound/items/{action_id}",
            delete(remove_claim_outbound_item),
        )
        .route(
            "/admin/claims/{id}/outbound/shipping-method",
            post(add_claim_outbound_shipping),
        )
        .route("/admin/returns", get(list_returns).post(request_return))
        .route("/admin/returns/{id}", get(get_return))
        .route("/admin/returns/{id}/items", get(return_items))
        .route("/admin/returns/{id}/receive", post(receive_return))
        .route(
            "/admin/returns/{id}/dismiss-items",
            post(dismiss_return_items),
        )
        .route("/admin/returns/{id}/cancel", post(cancel_return))
        .route("/admin/returns/{id}/request", post(confirm_return_request))
        .route(
            "/admin/returns/{id}/request-items",
            post(add_return_request_item),
        )
        .route(
            "/admin/returns/{id}/request-items/{action_id}",
            delete(remove_return_request_item),
        )
        .route(
            "/admin/returns/{id}/receive-items",
            post(add_return_receive_item),
        )
        .route(
            "/admin/returns/{id}/receive-items/{action_id}",
            delete(remove_return_receive_item),
        )
        .route(
            "/admin/returns/{id}/shipping-method",
            post(add_return_shipping),
        )
        .route(
            "/admin/returns/{id}/shipping-method/{action_id}",
            delete(remove_return_shipping),
        )
        .route(
            "/admin/return-reasons",
            get(list_return_reasons).post(create_return_reason),
        )
        .route(
            "/admin/return-reasons/{id}/translations",
            get(list_return_reason_translations).post(put_return_reason_translation),
        )
        .route(
            "/admin/return-reasons/{id}/translations/{locale}",
            get(localised_return_reason).delete(remove_return_reason_translation),
        )
        .route("/admin/returns/{id}/withdrawal", post(notify_withdrawal))
        .route(
            "/admin/selling-plan-groups/{id}/plans",
            get(list_plans).post(create_plan),
        )
        .route(
            "/admin/selling-plans/{id}/variants",
            post(attach_plan_variant),
        )
        .route(
            "/admin/products/{id}/images",
            get(list_images).post(add_image),
        )
        .route(
            "/admin/products/{id}/options",
            get(option_matrix).post(add_option),
        )
        .route(
            "/admin/product-variants/{id}/images",
            get(list_variant_images).post(attach_variant_image),
        )
        .route(
            "/admin/product-variants/{id}/options",
            get(variant_options).post(set_variant_options),
        )
        .route("/admin/product-options/{id}/values", post(add_option_value))
        .route(
            "/admin/product-categories/{id}/subtree",
            get(category_subtree),
        )
        .route("/admin/product-categories/{id}/move", post(move_category))
        .route("/admin/inventory-items/{id}/tracking", patch(set_tracking))
        .route(
            "/admin/inventory-items/{id}/lots",
            get(list_lots).post(receive_lot),
        )
        .route("/admin/inventory-lots/{id}/orders", get(orders_for_lot))
        .route(
            "/admin/inventory-lots/{id}/reservations",
            post(reserve_from_lot),
        )
        .route(
            "/admin/products/{id}/variants",
            get(list_variants).post(create_variant),
        )
        .route("/admin/price-sets", post(create_price_set))
        .route(
            "/admin/product-variants/{id}/price-set",
            post(link_variant_price_set),
        )
        .route("/admin/prices", post(add_price))
        .route(
            "/admin/inventory-items/{id}/location-levels",
            get(list_levels).post(set_stock),
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
        .route("/admin/payment-webhooks", get(pending_callbacks))
        .route("/admin/payment-webhooks/{id}/apply", post(apply_callback))
        .route(
            "/admin/payment-webhooks/{id}/processed",
            post(callback_processed),
        )
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
/// `Actor::Staff` carrying whoever cleared the gate, so an audit row can say
/// who changed a price. It was the nil uuid for every request until operators
/// existed, and still is for an `ADMIN_TOKEN` one — see [`Caller::actor_id`].
fn ctx_for<'a>(state: &'a AppState, caller: &Caller) -> Ctx<'a> {
    Ctx::new(
        state.scope,
        Actor::Staff {
            id: caller.actor_id(),
        },
        state.host.as_ref() as &dyn Host,
    )
}

/// What stands between a stranger and the back office.
///
/// Two credentials, one door. A session token belongs to a person, expires,
/// and can be revoked; `ADMIN_TOKEN` belongs to nobody, never expires, and is
/// how the first operator is made and how a shop that lost every password
/// gets back in. Whichever cleared the gate is put on the request as a
/// [`Caller`], so `ctx_for` can name a person in the audit row instead of the
/// nil uuid every request used to carry.
///
/// `ADMIN_TOKEN` is compared in constant time — the same discipline tezgah's
/// own webhook signature checks use `subtle` for. A session token is looked
/// up by digest, so there is nothing to compare byte by byte.
pub async fn require_operator(
    State(gate): State<Gate>,
    mut request: Request,
    next: Next,
) -> Response {
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_owned);

    let Some(token) = presented else {
        return denied();
    };

    if let Some(expected) = gate.admin_token.as_deref()
        && crate::identity::is_admin_token(&token, expected)
    {
        request.extensions_mut().insert(Caller::AdminToken);
        return next.run(request).await;
    }

    // Shape first, so a wrong `ADMIN_TOKEN` — or anything else somebody
    // sends — is refused without a database round trip. A session token is
    // exactly two v4 uuids, hex, no dashes.
    if !looks_like_session(&token) {
        return denied();
    }

    match crate::identity::session_operator(&gate.pool, &token).await {
        Ok(Some(operator)) => {
            if let Some(refusal) = refuse_by_role(&request, operator.role) {
                return refusal;
            }
            request
                .extensions_mut()
                .insert(Caller::Session { operator, token });
            next.run(request).await
        }
        Ok(None) => denied(),
        Err(err) => ApiError::from(err).into_response(),
    }
}

/// What the route table already says this route asks for, looked up by the
/// pattern axum matched.
///
/// Built once. `tezgah::api::routes()` allocates 487 `Route`s and nothing
/// wants that per request.
fn declared_actions() -> &'static HashMap<(&'static str, &'static str), Action> {
    static ACTIONS: OnceLock<HashMap<(&'static str, &'static str), Action>> = OnceLock::new();
    ACTIONS.get_or_init(|| {
        tezgah::api::routes()
            .into_iter()
            .map(|route| ((route.method.as_str(), route.path), route.action))
            .collect()
    })
}

/// Authorization, at the door.
///
/// The `Action` comes from `tezgah::api::routes()` — the same table the
/// OpenAPI document and the permission matrix read — so a role is checked
/// against what the route declares rather than against a second list kept
/// here and drifting from it.
///
/// This is coarser than an `Authorizer` and is not a replacement for one. It
/// answers "may this person refund anything at all", not "may this person
/// refund this order". The second question is what `tezgah::ports::Authorizer`
/// is for, and `ServerHost` still answers it by granting everything.
///
/// A path this binary binds that the table does not declare — its own
/// `/auth/*` and `/admin/operators*` — is not covered here. Those check the
/// role themselves, in `http::auth`, because what they need is not one of
/// tezgah's five actions.
fn refuse_by_role(request: &Request, role: Role) -> Option<Response> {
    let matched = request.extensions().get::<MatchedPath>()?;
    let action = declared_actions().get(&(request.method().as_str(), matched.as_str()))?;

    if role.may(*action) {
        return None;
    }

    Some(
        (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": {
                    "code": "denied",
                    "message": format!(
                        "a {} may not {} — ask an owner",
                        role.as_str(),
                        format!("{action:?}").to_lowercase()
                    ),
                }
            })),
        )
            .into_response(),
    )
}

fn looks_like_session(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// What `require_operator` needs to answer, and nothing else — the pool to
/// look a session up in, and the shared secret when there is one.
#[derive(Clone, Debug)]
pub struct Gate {
    pub pool: sqlx::PgPool,
    pub admin_token: Option<Arc<str>>,
}

fn denied() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": {
                "code": "denied",
                "message": "sign in at POST /auth/session, or send the admin token as \"authorization: Bearer <token>\"",
            }
        })),
    )
        .into_response()
}

async fn list_products(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::ListProducts>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::ProductView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_products(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let product = admin_catalogue::get_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(product))
}

async fn update_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::UpdateProduct>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let product = admin_catalogue::update_product(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(product))
}

async fn delete_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::delete_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_orders(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::ListOrders>,
) -> Result<Json<tezgah::page::Page<admin_order::OrderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_orders(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_order(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
) -> Result<Json<admin_order::OrderView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let order = admin_order::get_order(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(order))
}

async fn list_inventory_items(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::InventoryItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_inventory_items(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_inventory_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
) -> Result<Json<admin_catalogue::InventoryItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let item = admin_catalogue::get_inventory_item(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(item))
}

async fn delete_inventory_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::delete_inventory_item(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_customers(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::ListCustomers>,
) -> Result<Json<tezgah::page::Page<admin_rest::CustomerView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_customers(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_customer(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<Json<admin_rest::CustomerView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let customer = admin_rest::get_customer(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(customer))
}

async fn update_customer(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
    Json(body): Json<admin_rest::UpdateCustomer>,
) -> Result<Json<admin_rest::CustomerView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let customer = admin_rest::update_customer(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(customer))
}

async fn delete_customer(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_rest::delete_customer(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_promotions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::PromotionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_promotions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_promotion(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PromotionId>,
) -> Result<Json<admin_rest::PromotionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let promotion = admin_rest::get_promotion(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(promotion))
}

async fn update_promotion(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PromotionId>,
    Json(body): Json<admin_rest::UpdatePromotion>,
) -> Result<Json<admin_rest::PromotionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let promotion = admin_rest::update_promotion(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(promotion))
}

async fn delete_promotion(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PromotionId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_rest::delete_promotion(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<subscription::List>,
) -> Result<Json<tezgah::page::Page<subscription::SubscriptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = subscription::list_subscriptions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
) -> Result<Json<subscription::ContractView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let contract = subscription::get_subscription(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(contract))
}

async fn list_regions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::RegionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_regions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_region(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<RegionId>,
) -> Result<Json<admin_rest::RegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let region = admin_rest::get_region(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn update_region(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<RegionId>,
    Json(body): Json<admin_rest::UpdateRegion>,
) -> Result<Json<admin_rest::RegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let region = admin_rest::update_region(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn list_sales_channels(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::SalesChannelView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_sales_channels(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_sales_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SalesChannelId>,
) -> Result<Json<admin_rest::SalesChannelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let channel = admin_rest::get_sales_channel(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(channel))
}

async fn update_sales_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SalesChannelId>,
    Json(body): Json<admin_rest::UpdateSalesChannel>,
) -> Result<Json<admin_rest::SalesChannelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let channel = admin_rest::update_sales_channel(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(channel))
}

async fn delete_sales_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SalesChannelId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_rest::delete_sales_channel(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_currencies(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<Vec<admin_rest::CurrencyView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let currencies = admin_rest::list_currencies(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(currencies))
}

async fn create_currency(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_rest::CreateCurrency>,
) -> Result<Json<admin_rest::CurrencyView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let currency = admin_rest::create_currency(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(currency))
}

async fn create_region(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_rest::CreateRegion>,
) -> Result<Json<admin_rest::RegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let region = admin_rest::create_region(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn create_sales_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_rest::CreateSalesChannel>,
) -> Result<Json<admin_rest::SalesChannelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let channel = admin_rest::create_sales_channel(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(channel))
}

async fn list_publishable_keys(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::PublishableKeyView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_publishable_keys(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_publishable_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_rest::CreatePublishableKey>,
) -> Result<Json<admin_rest::IssuedKeyView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let key = admin_rest::create_publishable_key(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(key))
}

async fn list_stock_locations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::StockLocationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_stock_locations(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_stock_location(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::CreateStockLocation>,
) -> Result<Json<admin_catalogue::StockLocationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let location = admin_catalogue::create_stock_location(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(location))
}

async fn update_stock_location(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<StockLocationId>,
    Json(body): Json<admin_catalogue::RenameStockLocation>,
) -> Result<Json<admin_catalogue::StockLocationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let location = admin_catalogue::rename_stock_location(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(location))
}

async fn delete_stock_location(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<StockLocationId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::delete_stock_location(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A page of variants flat enough to write as CSV, and the same shape back.
///
/// The export and the import name the same columns on purpose: a shop's way
/// of changing four hundred prices is to take the page out, edit it, and put
/// it back. That only works if what comes out goes in.
async fn export_products(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::ExportQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::ProductExportView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::export_products(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn batch_products(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::ImportProductsBody>,
) -> Result<Json<admin_catalogue::ImportResultView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let result = admin_catalogue::batch_products(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn batch_prices(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::UpdatePricesBody>,
) -> Result<Json<admin_catalogue::BatchResultView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let result = admin_catalogue::batch_prices(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn batch_stock_levels(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::SetStockLevelsBody>,
) -> Result<Json<admin_catalogue::BatchResultView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let result = admin_catalogue::batch_stock_levels(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(result))
}

async fn create_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::CreateProduct>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let product = admin_catalogue::create_product(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(product))
}

async fn create_variant(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::CreateVariant>,
) -> Result<Json<admin_catalogue::VariantView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let variant = admin_catalogue::create_variant(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(variant))
}

async fn create_price_set(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<admin_catalogue::PriceSetView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let set = admin_catalogue::create_price_set(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(set))
}

async fn link_variant_price_set(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::LinkPriceSet>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::link_variant_price_set(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn add_price(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::AddPrice>,
) -> Result<Json<admin_catalogue::PriceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let price = admin_catalogue::add_price(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(price))
}

async fn create_inventory_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_catalogue::CreateInventoryItem>,
) -> Result<Json<admin_catalogue::InventoryItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let item = admin_catalogue::create_inventory_item(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(item))
}

/// An exchange: the same two legs a claim has, for a different reason — the
/// shopper wants something else rather than something working. And an order
/// edit, which is the third shape of the same idea: a change proposed against
/// an order, confirmed or declined as a whole.
async fn list_exchanges(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ExchangeView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::list_exchanges(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn request_exchange(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_order::RequestExchange>,
) -> Result<Json<admin_order::ExchangeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::request_exchange(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn get_exchange(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
) -> Result<Json<admin_order::ExchangeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::get_exchange(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn exchange_actions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
) -> Result<Json<admin_order::ChangeDetailView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::exchange_actions(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn cancel_exchange(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
) -> Result<Json<admin_order::ExchangeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::cancel_exchange(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn confirm_exchange_request(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
) -> Result<Json<admin_order::ExchangeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::confirm_exchange_request(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_exchange_inbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_exchange_inbound_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_exchange_inbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ExchangeId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_exchange_inbound_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_exchange_inbound_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_exchange_inbound_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_exchange_outbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_exchange_outbound_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_exchange_outbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ExchangeId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_exchange_outbound_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_exchange_outbound_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ExchangeId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_exchange_outbound_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn get_order_edit(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderChangeId>,
) -> Result<Json<admin_order::ChangeDetailView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::get_order_edit(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// `Option<Json<..>>`, because this is a `DELETE` and most clients send one
/// with no body at all. `DeclineChange`'s only field is a reason and it is
/// optional, so a bodyless decline is a decline with no reason given —
/// requiring a body would refuse the ordinary case to keep the extractor
/// tidy.
async fn decline_order_edit(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderChangeId>,
    body: Option<Json<admin_order::DeclineChange>>,
) -> Result<Json<admin_order::ChangeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let given = body.map(|Json(body)| body).unwrap_or_default();
    let view = admin_order::decline_order_edit(&mut tx, &ctx, id, given).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn confirm_order_edit(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderChangeId>,
) -> Result<Json<admin_order::OrderView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::confirm_order_edit(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_order_edit_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderChangeId>,
    Json(body): Json<admin_order::AddItemAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_order_edit_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_order_edit_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(OrderChangeId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_order_edit_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_order_edit_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderChangeId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_order_edit_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_order_edit_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(OrderChangeId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_order_edit_shipping(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

/// A claim: something arrived broken or wrong, and putting it right is two
/// movements at once — what comes back and what goes out. Fifteen routes,
/// declared and bound by nothing.
async fn list_claims(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ClaimView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::list_claims(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn request_claim(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_order::RequestClaim>,
) -> Result<Json<admin_order::ClaimView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::request_claim(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn get_claim(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
) -> Result<Json<admin_order::ClaimView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::get_claim(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn claim_actions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
) -> Result<Json<admin_order::ChangeDetailView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::claim_actions(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn claim_lines(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
) -> Result<Json<Vec<admin_order::ClaimItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::claim_lines(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn cancel_claim(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
) -> Result<Json<admin_order::ClaimView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::cancel_claim(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn confirm_claim_request(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
) -> Result<Json<admin_order::ClaimView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::confirm_claim_request(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_claim_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_claim_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_claim_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ClaimId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_claim_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_claim_inbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_claim_inbound_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_claim_inbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ClaimId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_claim_inbound_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_claim_inbound_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_claim_inbound_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_claim_outbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_claim_outbound_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_claim_outbound_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ClaimId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_claim_outbound_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_claim_outbound_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ClaimId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_claim_outbound_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// A return, from the request to the parcel arriving.
///
/// Twenty routes, all declared since the order domain was written and bound
/// by nothing — so a shop could take an order and had no way to take one
/// back, which is the half of commerce nobody advertises and everybody needs.
async fn list_returns(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ReturnView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_returns(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn request_return(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_order::RequestReturn>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::request_return(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn get_return(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::get_return(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn return_items(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
) -> Result<Json<Vec<admin_order::ReturnItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_order::return_items(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn receive_return(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
    Json(body): Json<admin_order::ReceiveReturn>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::receive_return(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// What came back damaged or wrong. Its own route rather than a flag on
/// receiving, because dismissing an item is a decision somebody makes about
/// goods in front of them.
async fn dismiss_return_items(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
    Json(body): Json<admin_order::ReceiveReturn>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::dismiss_return_items(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn cancel_return(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::cancel_return(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn confirm_return_request(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
) -> Result<Json<admin_order::ReturnView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::confirm_return_request(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_return_request_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_return_request_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_return_request_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ReturnId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_return_request_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_return_receive_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
    Json(body): Json<admin_order::LineQuantity>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_return_receive_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_return_receive_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ReturnId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_return_receive_item(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn add_return_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
    Json(body): Json<admin_order::AddShippingAction>,
) -> Result<Json<admin_order::ChangeActionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::add_return_shipping(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_return_shipping(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, action_id)): Path<(ReturnId, uuid::Uuid)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_return_shipping(&mut tx, &ctx, id, action_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn list_return_reasons(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ReasonView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_return_reasons(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_return_reason(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Json(body): Json<admin_order::NewReason>,
) -> Result<Json<admin_order::ReasonView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::create_return_reason(&mut tx, &ctx, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_return_reason_translations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<Vec<admin_order::ReturnReasonTranslationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_order::list_return_reason_translations(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn put_return_reason_translation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<admin_order::PutReturnReasonTranslation>,
) -> Result<Json<admin_order::ReturnReasonTranslationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::put_return_reason_translation(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// A shopper is told why they may send something back in their own language,
/// so the reason a shop writes has one text per locale.
async fn localised_return_reason(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, locale)): Path<(uuid::Uuid, String)>,
) -> Result<Json<admin_order::LocalisedReturnReasonView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::localised_return_reason(&mut tx, &ctx, id, &locale).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn remove_return_reason_translation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, locale)): Path<(uuid::Uuid, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::remove_return_reason_translation(&mut tx, &ctx, id, &locale).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

/// The last of the sub-routes that had a function waiting and no path to it:
/// what a campaign spends and covers, who is in a group, what a shop hands a
/// customer who asks for their data, where stock moved, what a price list
/// narrows to, what a product is called in another language, what a bundle is
/// made of, which channels a storefront key may see, and the two acts that
/// close a reservation and a return.
async fn set_campaign_budget(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CampaignId>,
    Json(body): Json<admin_rest::SetBudget>,
) -> Result<Json<admin_rest::CampaignBudgetView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::set_campaign_budget(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_campaign_promotion(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CampaignId>,
    Json(body): Json<admin_rest::AttachPromotion>,
) -> Result<Json<admin_rest::PromotionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::add_campaign_promotion(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_group_members(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerGroupId>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::CustomerView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_group_members(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn add_group_member(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerGroupId>,
    Json(body): Json<admin_rest::GroupMember>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_rest::add_group_member(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "added": true })))
}

/// Everything this shop holds about one person, as they are entitled to ask
/// for it. A `GET`, because it answers rather than changes.
async fn export_customer(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::export_customer(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_stock_transfers(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::StockTransferView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_stock_transfers(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn transfer_stock(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Json(body): Json<admin_catalogue::TransferStock>,
) -> Result<Json<admin_catalogue::StockTransferView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::transfer_stock(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_price_list_rule(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PriceListId>,
    Json(body): Json<admin_catalogue::AddPriceListRule>,
) -> Result<Json<admin_catalogue::PriceListRuleView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::add_price_list_rule(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_translations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::TranslationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_translations(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn put_translation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::PutTranslation>,
) -> Result<Json<admin_catalogue::TranslationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::put_translation(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_category_translations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CategoryId>,
) -> Result<Json<Vec<admin_catalogue::CategoryTranslationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_category_translations(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn put_category_translation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CategoryId>,
    Json(body): Json<admin_catalogue::PutCategoryTranslation>,
) -> Result<Json<admin_catalogue::CategoryTranslationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::put_category_translation(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn set_bundle_price(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::SetBundlePrice>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::set_bundle_price(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "set": true })))
}

async fn list_variant_inventory_items(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<admin_catalogue::VariantInventoryItemView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_variant_inventory_items(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn attach_inventory_item(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::AttachInventoryItem>,
) -> Result<Json<admin_catalogue::VariantInventoryItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::attach_inventory_item(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_key_sales_channels(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PublishableKeyId>,
) -> Result<Json<Vec<admin_rest::SalesChannelView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_rest::list_key_sales_channels(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn link_key_sales_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PublishableKeyId>,
    Json(body): Json<admin_rest::LinkSalesChannel>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_rest::link_key_sales_channel(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "linked": true })))
}

async fn fulfil_reservation(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReservationId>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::fulfil_reservation(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "fulfilled": true })))
}

async fn notify_withdrawal(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ReturnId>,
) -> Result<Json<agreement::WithdrawalNoticeView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = agreement::notify_withdrawal(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_plans(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SellingPlanGroupId>,
    Query(query): Query<subscription::List>,
) -> Result<Json<tezgah::page::Page<subscription::PlanView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = subscription::list_plans(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn create_plan(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SellingPlanGroupId>,
    Json(body): Json<subscription::CreatePlan>,
) -> Result<Json<subscription::PlanView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::create_plan(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn attach_plan_variant(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SellingPlanId>,
    Json(body): Json<subscription::AttachVariant>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    subscription::attach_variant(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "offered": true })))
}

/// What a product looks like and what makes one variant different from
/// another, plus the lots a shop has to be able to recall. Sixteen more that
/// were declared and bound by nothing.
async fn list_images(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::ImageView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_images(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn add_image(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::AddImage>,
) -> Result<Json<admin_catalogue::ImageView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::add_image(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn option_matrix(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::OptionMatrixView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::option_matrix(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn add_option(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::AddOption>,
) -> Result<Json<admin_catalogue::OptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::add_option(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn add_option_value(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OptionId>,
    Json(body): Json<admin_catalogue::AddOptionValue>,
) -> Result<Json<admin_catalogue::OptionValueView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::add_option_value(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_variant_images(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<admin_catalogue::ImageView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_variant_images(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn attach_variant_image(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::AttachVariantImage>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::attach_image_to_variant(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "attached": true })))
}

async fn variant_options(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<admin_catalogue::OptionValueView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::variant_options(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn set_variant_options(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<admin_catalogue::SetVariantOptions>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::set_variant_options(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "set": true })))
}

async fn category_subtree(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CategoryId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::CategoryView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::category_subtree(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn move_category(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CategoryId>,
    Json(body): Json<admin_catalogue::MoveCategory>,
) -> Result<Json<admin_catalogue::CategoryView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::move_category(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Lots, and the two questions a recall asks: which orders had this lot, and
/// hold me some of it.
async fn set_tracking(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Json(body): Json<inventory_lot::SetTracking>,
) -> Result<Json<inventory_lot::TrackedItemView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = inventory_lot::set_tracking(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_lots(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Query(query): Query<inventory_lot::ListLots>,
) -> Result<Json<tezgah::page::Page<inventory_lot::LotView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = inventory_lot::list_lots(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn receive_lot(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Json(body): Json<inventory_lot::ReceiveLot>,
) -> Result<Json<inventory_lot::LotView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = inventory_lot::receive_lot(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn orders_for_lot(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryLotId>,
    Query(query): Query<inventory_lot::ListRecall>,
) -> Result<Json<tezgah::page::Page<inventory_lot::LotShipmentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = inventory_lot::orders_for_lot(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn reserve_from_lot(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryLotId>,
    Json(body): Json<inventory_lot::ReserveFromLot>,
    // `admin_catalogue`'s, not `inventory_lot`'s: reserving from a lot answers
    // the same view a reservation anywhere else does, and the lot module
    // imports it rather than declaring a second one.
) -> Result<Json<admin_catalogue::ReservationView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = inventory_lot::reserve_from_lot(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Seventeen more that were declared and bound by nothing: what an order's
/// buyer accepted and was invoiced, what a promotion is doing, where a
/// customer has things sent, and the four ways money or a key is taken back.
async fn list_invoices(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
) -> Result<Json<Vec<agreement::InvoiceView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = agreement::list_invoices(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn record_invoice(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
    Json(body): Json<agreement::RecordInvoice>,
) -> Result<Json<agreement::InvoiceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = agreement::record_invoice(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn order_agreements(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
) -> Result<Json<Vec<agreement::OrderAgreementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = agreement::order_agreements(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

/// When the buyer may still change their mind, which a shop is asked about
/// far more often than it is asked what the terms said.
async fn order_withdrawal(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
) -> Result<Json<Vec<agreement::WithdrawalView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = agreement::withdrawal_windows(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn set_promotion_status(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PromotionId>,
    Json(body): Json<admin_rest::SetStatus>,
) -> Result<Json<admin_rest::PromotionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::set_promotion_status(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn set_application_method(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PromotionId>,
    Json(body): Json<admin_rest::SetApplicationMethod>,
) -> Result<Json<admin_rest::ApplicationMethodView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::set_application_method(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_addresses(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::AddressView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_addresses(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn add_address(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
    Json(body): Json<admin_rest::WriteAddress>,
) -> Result<Json<admin_rest::AddressView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::add_address(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// Erasing on request. The orders stay and the person does not — which is
/// what makes this a route rather than a delete.
async fn erase_customer(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<Json<admin_rest::CustomerView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::erase_customer(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn adjust_gift_card(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<GiftCardId>,
    Json(body): Json<credit::Adjustment>,
) -> Result<Json<credit::GiftCardView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = credit::adjust_gift_card(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn disable_gift_card(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<GiftCardId>,
) -> Result<Json<credit::GiftCardView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = credit::disable_gift_card(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn revoke_publishable_key(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PublishableKeyId>,
) -> Result<Json<admin_rest::PublishableKeyView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::revoke_publishable_key(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn list_region_countries(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<RegionId>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::RegionCountryView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_region_countries(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn add_region_country(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<RegionId>,
    Json(body): Json<admin_rest::AddRegionCountry>,
) -> Result<Json<admin_rest::RegionCountryView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_rest::add_region_country(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn get_location_address(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<StockLocationId>,
) -> Result<Json<Option<admin_catalogue::StockLocationAddressView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::get_stock_location_address(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn set_location_address(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<StockLocationId>,
    Json(body): Json<admin_catalogue::StockLocationAddressIn>,
) -> Result<Json<admin_catalogue::StockLocationAddressView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::set_stock_location_address(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn revoke_exemption(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<uuid::Uuid>,
    Json(body): Json<tax_identity::RevokeExemption>,
) -> Result<Json<tax_identity::ExemptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = tax_identity::revoke_exemption(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// What a shop does to a contract after it exists.
///
/// Seven routes, all declared since subscriptions were written and bound by
/// nothing — so this panel could list a subscription and offer no way to stop
/// it, which is the one thing a shop is asked to do about one.
///
/// Two of the nine stay unbound, and not by choice. `renew` and `card` take a
/// `subscription::Renewals`, which takes a `RecurringProvider`, and this
/// binary has none: charging a card a shopper left on file means naming which
/// card, and kasapay 0.0.5 has no field for one — `src/provider.rs` carries
/// that in full, and it is the same reason `host::Dispatcher` renews nothing.
/// Binding them would mean building a `Renewals` out of a provider that
/// cannot do the one thing it is for.
async fn subscription_events(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
    Query(query): Query<subscription::List>,
) -> Result<Json<tezgah::page::Page<subscription::EventView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = subscription::list_events(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn cancel_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
    Json(body): Json<subscription::Cancel>,
) -> Result<Json<subscription::SubscriptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::cancel_subscription(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn pause_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
    Json(body): Json<subscription::Pause>,
) -> Result<Json<subscription::SubscriptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::pause_subscription(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn resume_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
) -> Result<Json<subscription::SubscriptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::resume_subscription(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn skip_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
) -> Result<Json<subscription::SubscriptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::skip_subscription(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn swap_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
    Json(body): Json<subscription::Swap>,
) -> Result<Json<subscription::SubscriptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = subscription::swap_subscription(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn deliver_subscription(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<SubscriptionId>,
    Json(body): Json<subscription::Deliver>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let order = subscription::deliver_subscription(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "order_id": order })))
}

/// The five that move a product's status.
///
/// Each is its own route rather than a field on `PATCH`, and the crate is
/// emphatic about why: a status is moved by the transition that is allowed,
/// not by writing the word. Declared since the catalogue was written and
/// bound by nothing, so a draft could be made here and never published.
async fn publish_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::publish_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn archive_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::archive_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn submit_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::submit_product_for_review(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn approve_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::approve_product(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn reject_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::RejectProduct>,
) -> Result<Json<admin_catalogue::ProductView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_catalogue::reject_product(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(view))
}

/// What a product is filed under, and what sells it. Three lists and their
/// two writes each — all declared in the route table and, until now, drawn
/// and reachable by nothing.
async fn list_product_tags(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::TagView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_product_tags(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn tag_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::AttachTag>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::tag_product(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "tagged": true })))
}

async fn untag_product(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, tag_id)): Path<(ProductId, ProductTagId)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::untag_product(&mut tx, &ctx, id, tag_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "untagged": true })))
}

async fn list_product_categories(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::CategoryView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_product_categories(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn add_product_to_category(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::AttachCategory>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::add_product_to_category(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "filed": true })))
}

async fn remove_product_from_category(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, category_id)): Path<(ProductId, CategoryId)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::remove_product_from_category(&mut tx, &ctx, id, category_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn list_product_channels(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
) -> Result<Json<Vec<admin_catalogue::ProductChannelView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rows = admin_catalogue::list_product_channels(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rows))
}

async fn add_product_to_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Json(body): Json<admin_catalogue::AttachChannel>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::add_product_to_channel(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "listed": true })))
}

async fn remove_product_from_channel(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, sales_channel_id)): Path<(ProductId, SalesChannelId)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_catalogue::remove_product_from_channel(&mut tx, &ctx, id, sales_channel_id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "delisted": true })))
}

/// A product's variants.
///
/// Declared with a paging query since the catalogue was written and bound by
/// nothing, so the panel could show a product and not what a shop actually
/// sells — the variant is the thing with a price, a SKU and stock behind it.
async fn list_variants(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ProductId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::VariantView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_variants(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

/// What is where, for one item.
///
/// Declared in `tezgah::api::routes()` since the inventory domain was written
/// and bound by nothing until now, so the panel could show an item without
/// ever saying how many there were.
async fn list_levels(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::InventoryLevelView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_levels(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn set_stock(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<InventoryItemId>,
    Json(body): Json<admin_catalogue::SetStock>,
) -> Result<Json<admin_catalogue::InventoryLevelView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let level = admin_catalogue::set_stock(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(level))
}

async fn get_basket(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderBasketId>,
) -> Result<Json<order_basket::BasketView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let basket = order_basket::get_basket(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(basket))
}

async fn basket_orders(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderBasketId>,
    Query(query): Query<order_basket::ListBasketOrders>,
) -> Result<Json<tezgah::page::Page<admin_order::OrderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = order_basket::basket_orders(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn basket_carts(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderBasketId>,
    Query(query): Query<order_basket::ListBasketOrders>,
) -> Result<Json<tezgah::page::Page<store_api::CartView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = order_basket::basket_carts(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_workflow_runs(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::ListWorkflowRuns>,
) -> Result<Json<tezgah::page::Page<admin_rest::WorkflowRunSummaryView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_workflow_runs(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_workflow_run(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<WorkflowRunId>,
) -> Result<Json<admin_rest::WorkflowRunView>, ApiError> {
    let ctx = ctx_for(&state, &caller);
    let run = admin_rest::get_workflow_run(&state.pool, &ctx, id).await?;
    Ok(Json(run))
}

async fn list_workflow_run_steps(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<WorkflowRunId>,
) -> Result<Json<Vec<admin_rest::WorkflowStepView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let steps = admin_rest::list_workflow_run_steps(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(steps))
}

async fn list_workflow_dead_letters(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::WorkflowDeadLetterView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_workflow_dead_letters(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn commission_rules(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::CommissionRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = payout::commission_rules(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn order_payout_lines(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::PayoutLineView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = payout::order_payout_lines(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_payouts(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<payout::ListQuery>,
) -> Result<Json<tezgah::page::Page<payout::PayoutView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = payout::payouts(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn payout_balance(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(currency_code): Path<String>,
) -> Result<Json<payout::BalanceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
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
    Extension(caller): Extension<Caller>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::FulfillmentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::order_fulfillments(&mut tx, &ctx, order_id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn order_shipping_options(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<CountryQuery>,
) -> Result<Json<Vec<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let options =
        admin_order::order_shipping_options(&mut tx, &ctx, order_id, &query.country_code).await?;
    tx.commit().await?;
    Ok(Json(options))
}

async fn return_shipping_options(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(order_id): Path<OrderId>,
    Query(query): Query<CountryQuery>,
) -> Result<Json<Vec<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let options =
        admin_order::return_shipping_options(&mut tx, &ctx, order_id, &query.country_code).await?;
    tx.commit().await?;
    Ok(Json(options))
}

async fn get_fulfillment(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((order_id, id)): Path<(OrderId, FulfillmentId)>,
) -> Result<Json<admin_order::FulfillmentDetailView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let fulfillment = admin_order::get_fulfillment(&mut tx, &ctx, order_id, id).await?;
    tx.commit().await?;
    Ok(Json(fulfillment))
}

async fn list_fulfillment_sets(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::FulfillmentSetView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_fulfillment_sets(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn service_zones(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<FulfillmentSetId>,
) -> Result<Json<Vec<admin_order::ServiceZoneView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let zones = admin_order::service_zones(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(zones))
}

async fn fulfillment_providers(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<Vec<admin_order::ProviderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let providers = admin_order::fulfillment_providers(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(providers))
}

async fn list_shipping_options(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingOptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_shipping_options(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_shipping_option(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ShippingOptionId>,
) -> Result<Json<admin_order::ShippingOptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let option = admin_order::get_shipping_option(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(option))
}

async fn list_shipping_option_translations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ShippingOptionId>,
) -> Result<Json<Vec<admin_order::ShippingOptionTranslationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let translations = admin_order::list_shipping_option_translations(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(translations))
}

async fn localised_shipping_option(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path((id, locale)): Path<(ShippingOptionId, String)>,
) -> Result<Json<admin_order::LocalisedShippingOptionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let translation = admin_order::localised_shipping_option(&mut tx, &ctx, id, &locale).await?;
    tx.commit().await?;
    Ok(Json(translation))
}

async fn list_shipping_profiles(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingProfileView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_shipping_profiles(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_shipping_profile(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<ShippingProfileId>,
) -> Result<Json<admin_order::ShippingProfileView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let profile = admin_order::get_shipping_profile(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(profile))
}

async fn list_shipping_option_types(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ShippingOptionTypeView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_shipping_option_types(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_tax_regions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::List>,
) -> Result<Json<tezgah::page::Page<admin_rest::TaxRegionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_tax_regions(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_tax_region(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<TaxRegionId>,
) -> Result<Json<admin_rest::TaxRegionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let region = admin_rest::get_tax_region(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(region))
}

async fn list_tax_rates(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_rest::ListTaxRates>,
) -> Result<Json<tezgah::page::Page<admin_rest::TaxRateView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_rest::list_tax_rates(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_tax_rate(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<TaxRateId>,
) -> Result<Json<admin_rest::TaxRateView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rate = admin_rest::get_tax_rate(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rate))
}

async fn list_tax_rate_rules(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<TaxRateId>,
) -> Result<Json<Vec<admin_rest::TaxRateRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rules = admin_rest::list_tax_rate_rules(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rules))
}

async fn list_tax_registrations(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<Vec<tax_identity::RegistrationView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let registrations = tax_identity::list_registrations(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(registrations))
}

async fn list_customer_tax_ids(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<Json<Vec<tax_identity::TaxIdView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let ids = tax_identity::list_tax_ids(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(ids))
}

async fn list_customer_tax_exemptions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
) -> Result<Json<Vec<tax_identity::ExemptionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let exemptions = tax_identity::list_exemptions(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(exemptions))
}

async fn get_price_set(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PriceSetId>,
) -> Result<Json<admin_catalogue::PriceSetView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let set = admin_catalogue::get_price_set(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(set))
}

async fn list_prices(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PriceSetId>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::PriceView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_prices(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_bundle_components(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<admin_catalogue::BundleComponentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let components = admin_catalogue::list_bundle_components(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(components))
}

async fn bundle_price(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Query(query): Query<admin_catalogue::BundlePriceQuery>,
) -> Result<Json<admin_catalogue::BundlePriceView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let price = admin_catalogue::bundle_price(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(price))
}

async fn list_price_rules(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PriceId>,
) -> Result<Json<Vec<admin_catalogue::PriceRuleView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let rules = admin_catalogue::list_price_rules(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(rules))
}

async fn list_price_lists(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::ListQuery>,
) -> Result<Json<tezgah::page::Page<admin_catalogue::PriceListView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_catalogue::list_price_lists(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_price_list(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PriceListId>,
) -> Result<Json<admin_catalogue::PriceListView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let list = admin_catalogue::get_price_list(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(list))
}

async fn get_price_preference(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_catalogue::FindPricePreference>,
) -> Result<Json<Option<admin_catalogue::PricePreferenceView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let preference = admin_catalogue::get_price_preference(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(preference))
}

async fn list_payments(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::ListPayments>,
) -> Result<Json<tezgah::page::Page<admin_order::PaymentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_payments(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

/// What a provider sent and nothing has acted on yet.
async fn pending_callbacks(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::ListCallbacks>,
) -> Result<Json<tezgah::page::Page<admin_order::PendingCallbackView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::pending_callbacks(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

/// Acts on what a provider already did, against a row that is durable.
async fn apply_callback(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PaymentWebhookEventId>,
) -> Result<Json<admin_order::AppliedView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let view = admin_order::apply_callback(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn callback_processed(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PaymentWebhookEventId>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    admin_order::callback_processed(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "processed": true })))
}

async fn get_payment(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PaymentId>,
) -> Result<Json<admin_order::PaymentView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let payment = admin_order::get_payment(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(payment))
}

async fn payment_providers(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
) -> Result<Json<Vec<admin_order::ProviderView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let providers = admin_order::payment_providers(&mut tx, &ctx).await?;
    tx.commit().await?;
    Ok(Json(providers))
}

async fn get_payment_collection(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PaymentCollectionId>,
) -> Result<Json<admin_order::CollectionView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let collection = admin_order::get_payment_collection(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(collection))
}

async fn payment_sessions(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<PaymentCollectionId>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::SessionView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::payment_sessions(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_refund_reasons(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<admin_order::Listing>,
) -> Result<Json<tezgah::page::Page<admin_order::ReasonView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = admin_order::list_refund_reasons(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_gift_cards(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::GiftCardView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = credit::list_gift_cards(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_gift_card(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<GiftCardId>,
) -> Result<Json<credit::GiftCardView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let card = credit::get_gift_card(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(card))
}

async fn gift_card_movements(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<GiftCardId>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::CreditMovementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = credit::gift_card_movements(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn get_store_credit(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<CustomerId>,
    Query(query): Query<credit::BalanceQuery>,
) -> Result<Json<credit::StoreCreditView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let balance = credit::get_store_credit(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(balance))
}

async fn store_credit_movements(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<StoreCreditId>,
    Query(query): Query<credit::List>,
) -> Result<Json<tezgah::page::Page<credit::CreditMovementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = credit::store_credit_movements(&mut tx, &ctx, id, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}

async fn list_order_entitlements(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
) -> Result<Json<Vec<digital::EntitlementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let entitlements = digital::list_order_entitlements(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(entitlements))
}

async fn revoke_entitlements(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<OrderId>,
    Json(body): Json<digital::RevokeEntitlements>,
) -> Result<Json<Vec<digital::EntitlementView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let entitlements = digital::revoke_entitlements(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(entitlements))
}

async fn list_content(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
) -> Result<Json<Vec<digital::ContentView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let content = digital::list_content(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(Json(content))
}

async fn put_content(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<VariantId>,
    Json(body): Json<digital::PutContent>,
) -> Result<Json<digital::ContentView>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let content = digital::put_content(&mut tx, &ctx, id, body).await?;
    tx.commit().await?;
    Ok(Json(content))
}

async fn delete_content(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Path(id): Path<DigitalContentId>,
) -> Result<StatusCode, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    digital::delete_content(&mut tx, &ctx, id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_carts(
    State(state): State<AppState>,
    Extension(caller): Extension<Caller>,
    Query(query): Query<order_basket::ListCarts>,
) -> Result<Json<tezgah::page::Page<store_api::CartView>>, ApiError> {
    let mut tx = begin(&state.pool, state.scope).await?;
    let ctx = ctx_for(&state, &caller);
    let page = order_basket::list_carts(&mut tx, &ctx, query).await?;
    tx.commit().await?;
    Ok(Json(page))
}
