//! "Every route declares its permission, and a matrix test proves it."
//!
//! Two halves, because the claim has two halves.
//!
//! The first reads [`tezgah::api::routes`] and checks the table against itself:
//! a reading route asks to read, a deleting route asks to delete, a storefront
//! route never asks to move money, and every route is on the surface its path
//! says it is. That is a property of the table and needs no database.
//!
//! The second calls handlers with a host that refuses everything and insists
//! each one comes back denied. That is the half a table alone cannot prove:
//! that the declaration is not decoration. A route whose handler never asked
//! would answer instead of refusing, or — the failure mode this file exists
//! to catch — would look for a row before it asks and answer `not_found`
//! instead of `denied`, which tells an unauthorised caller something about a
//! resource it should never have heard of.
//!
//! Every route in [`routes()`] is either called here and asserted denied, or
//! named in [`TOLERATED`] with the reason it is not — the completeness check
//! at the bottom of `every_route_is_denied_by_a_host_that_refuses_everything`
//! fails the build the day a route is added to the table and forgotten here.
//! Most of `TOLERATED` is the failure mode above, caught but not fixed: a
//! handler whose permission needs an owner id only its own row carries loads
//! that row before it asks, so a synthetic id answers `not_found` rather
//! than `denied`. Existing rows stay protected — the permit check still
//! runs once the row is loaded — but the distinction between "does not
//! exist" and "exists, not yours" leaks to someone the crate's own rule
//! says should never get an answer at all. See #151 and #152.

mod common;

use common::{Doorman, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::api::{
    Method, Route, Surface, admin_catalogue, admin_order, admin_rest, agreement, credit, digital,
    inventory_lot, order_basket, payout, routes, store, subscription, tax_identity,
};
use tezgah::id::{
    AccountHolderId, AddressId, AgreementVersionId, CampaignId, CartCreditId, CartId, CategoryId,
    ClaimId, CollectionId, CommissionRuleId, CustomerGroupId, CustomerId, DigitalContentId,
    ExchangeId, FulfillmentId, FulfillmentSetId, GiftCardId, InventoryItemId, InventoryLotId,
    LineItemId, OptionId, OrderBasketId, OrderChangeId, OrderEntitlementId, OrderId,
    OrderInvoiceId, PaymentCollectionId, PaymentId, PaymentProviderId, PaymentWebhookEventId,
    PriceId, PriceListId, PriceSetId, ProductId, ProductImageId, ProductTagId, ProductTypeId,
    PromotionId, PublishableKeyId, RegionId, ReservationId, ReturnId, SalesChannelId,
    SellingPlanGroupId, SellingPlanId, ServiceZoneId, ShippingOptionId, ShippingProfileId,
    StockLocationId, StockTransferId, StoreCreditId, SubscriptionId, TaxRateId, TaxRegionId,
    VariantId, WorkflowRunId,
};
use tezgah::ports::{Action, Actor};

fn admin(route: &Route) -> bool {
    route.surface == Surface::Admin
}

#[test]
fn the_table_is_not_empty() {
    assert!(
        routes().len() > 40,
        "the route table has shrunk to nothing; something is not being registered"
    );
}

#[test]
fn a_reading_route_asks_only_to_read() {
    for route in routes() {
        if route.method == Method::Get {
            assert_eq!(
                route.action,
                Action::View,
                "{} {} reads but asks for {:?}",
                route.method.as_str(),
                route.path,
                route.action
            );
        }
    }
}

#[test]
fn a_deleting_route_asks_to_delete() {
    for route in routes() {
        if route.method == Method::Delete {
            assert!(
                matches!(route.action, Action::Delete | Action::Write),
                "{} asks for {:?}, which is not a permission to remove anything",
                route.path,
                route.action
            );
        }
    }
}

/// The one power a storefront must never have. Capture, refund and cancel are
/// the back office's, and a shopper reaching one would be moving money in
/// somebody else's shop.
#[test]
fn no_storefront_route_may_move_money() {
    for route in routes() {
        if route.surface == Surface::Store {
            assert_ne!(
                route.action,
                Action::Settle,
                "{} is on the storefront and asks to settle money",
                route.path
            );
        }
    }
}

#[test]
fn a_route_lives_under_the_prefix_its_surface_claims() {
    for route in routes() {
        let expected = match route.surface {
            Surface::Store => "/store/",
            Surface::Admin => "/admin/",
            // Neither a shopper's nor a back office's, and its prefix says so:
            // what reaches it is an outside system posting where it was told
            // to, authenticated by a signature the host checks.
            Surface::Webhook => "/webhooks/",
        };
        assert!(
            route.path.starts_with(expected),
            "{} is on {:?} but does not start with {expected}",
            route.path,
            route.surface
        );
    }
}

#[test]
fn every_route_carries_a_domain_and_a_summary() {
    for route in routes() {
        assert!(
            !route.domain.is_empty()
                && route
                    .domain
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
            "{} has {:?} for a domain tag",
            route.path,
            route.domain
        );
        assert!(
            !route.summary.is_empty() && !route.summary.ends_with('.'),
            "{} has {:?} for a summary; one line, no full stop",
            route.path,
            route.summary
        );
    }
}

/// A domain is an OpenAPI tag, and a tag nobody else uses is usually a typo.
#[test]
fn every_admin_domain_is_one_the_crate_has() {
    let known = [
        "catalogue",
        "cart",
        "checkout",
        "credit",
        "customer",
        "digital",
        "fulfilment",
        "inventory",
        "order",
        "order_basket",
        "payment",
        "payout",
        "pricing",
        "promotion",
        "store",
        "subscription",
        "tax",
        "workflow",
    ];

    for route in routes().iter().filter(|route| admin(route)) {
        assert!(
            known.contains(&route.domain),
            "{} is tagged {:?}, which is not a module of this crate",
            route.path,
            route.domain
        );
    }
}

/// Every route the matrix below does not call, and why not.
///
/// This list may only shrink. An entry earns its place by naming a real
/// obstacle — not "nobody got to it yet". What is left is a route whose input
/// cannot be built without a fixture this test does not have (a
/// `PaymentProvider`, a `RecurringProvider`) — the permit check itself is not
/// in question, only whether this file can reach it.
///
/// The other shape this list used to hold — a handler that loaded its row
/// before asking permission, because the permission it needed depended on a
/// parent id only the row carried, so a synthetic id answered `not_found`
/// before permission ever entered it — is gone: `order::read`,
/// `order::read_change`, `order::read_return`, `order_basket::get`,
/// `subscription::get` and `admin_order.rs`'s own `read_return`,
/// `read_exchange`, `read_claim`, `read_change`, `open_change` and
/// `open_edit` now ask on the miss, before answering. See
/// productdevbook/tezgah#151 and productdevbook/tezgah#152.
static TOLERATED: &[(Method, &str, &str)] = &[
    // Needs a live `RecurringProvider` (kasapay-shaped) to construct
    // `subscription::Renewals`; the permit check inside `subscription::get`
    // still runs first, this file just cannot build the argument after it.
    (
        Method::Post,
        "/admin/subscriptions/{id}/renew",
        "needs a RecurringProvider fixture this test does not build",
    ),
    // Same gap as `renew` above: `subscription::repoint_card` takes a
    // `&Renewals` too, since a `past_due` contract retries through the
    // identical call.
    (
        Method::Post,
        "/admin/subscriptions/{id}/card",
        "needs a RecurringProvider fixture this test does not build",
    ),
    (
        Method::Post,
        "/store/subscriptions/{id}/card",
        "needs a RecurringProvider fixture this test does not build",
    ),
    // Needs a live `PaymentProvider` to construct `checkout::Checkout`; same
    // shape of gap as `renew` above.
    (
        Method::Post,
        "/store/carts/{id}/complete",
        "needs a PaymentProvider fixture this test does not build",
    ),
];

fn tolerated(method: Method, path: &str) -> bool {
    TOLERATED.iter().any(|(m, p, _)| *m == method && *p == path)
}

/// A zero-amount `MoneyIn` in the shop's own currency, for calls that only
/// need to type-check on their way to being denied.
fn try_(amount: Decimal) -> admin_order::MoneyIn {
    admin_order::MoneyIn {
        amount,
        currency: "TRY".to_owned(),
    }
}

/// The half a table cannot prove: that every declared route's handler asks —
/// and asks before it does anything else that could answer a question the
/// permission check exists to gate.
///
/// One call per route rather than one per action per domain: the earlier,
/// smaller version of this test called one handler per domain and reasoned
/// that every other handler in the same domain shares the same permission
/// call. That reasoning missed `GET /admin/workflows-executions/{id}`, which
/// asked nothing at all — the route was the only place the question could be
/// put, and nobody had put it there. A rule that holds for most of a table
/// is not the same as a rule that holds for the table.
#[tokio::test]
async fn every_route_is_denied_by_a_host_that_refuses_everything() {
    let shop = Shop::open().await;
    let doorman = Doorman;
    let ctx = shop.ctx_as(
        Actor::Staff {
            id: uuid::Uuid::now_v7(),
        },
        &doorman,
    );
    let mut tx = shop.begin().await;

    let mut refused: Vec<&'static str> = Vec::new();
    let mut allowed: Vec<String> = Vec::new();
    let mut covered: Vec<(Method, &'static str)> = Vec::new();

    macro_rules! denied {
        ($method:expr, $path:literal, $call:expr) => {{
            covered.push(($method, $path));
            match $call.await {
                Err(error) if error.is_denied() => refused.push($path),
                Err(error) => allowed.push(format!(
                    "{} {}: answered {:?} instead of denying",
                    stringify!($method),
                    $path,
                    error.code()
                )),
                Ok(_) => allowed.push(format!(
                    "{} {}: answered without asking",
                    stringify!($method),
                    $path
                )),
            }
        }};
    }

    // ----------------------------------------------------- admin_rest.rs ---
    denied!(
        Method::Get,
        "/admin/customers",
        admin_rest::list_customers(&mut tx, &ctx, admin_rest::ListCustomers::default())
    );
    denied!(
        Method::Post,
        "/admin/customers",
        admin_rest::create_customer(&mut tx, &ctx, admin_rest::CreateCustomer::default())
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}",
        admin_rest::get_customer(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Patch,
        "/admin/customers/{id}",
        admin_rest::update_customer(
            &mut tx,
            &ctx,
            CustomerId::new(),
            admin_rest::UpdateCustomer::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/customers/{id}",
        admin_rest::delete_customer(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}/export",
        admin_rest::export_customer(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Post,
        "/admin/customers/{id}/erase",
        admin_rest::erase_customer(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}/addresses",
        admin_rest::list_addresses(
            &mut tx,
            &ctx,
            CustomerId::new(),
            admin_rest::List::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/customers/{id}/addresses",
        admin_rest::add_address(
            &mut tx,
            &ctx,
            CustomerId::new(),
            admin_rest::WriteAddress::default()
        )
    );
    denied!(
        Method::Patch,
        "/admin/customers/{id}/addresses/{address_id}",
        admin_rest::update_address(
            &mut tx,
            &ctx,
            AddressId::new(),
            admin_rest::WriteAddress::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/customers/{id}/addresses/{address_id}",
        admin_rest::delete_address(&mut tx, &ctx, AddressId::new())
    );
    denied!(
        Method::Get,
        "/admin/customer-groups",
        admin_rest::list_groups(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/customer-groups",
        admin_rest::create_group(
            &mut tx,
            &ctx,
            admin_rest::WriteGroup {
                name: String::new(),
                metadata: None
            }
        )
    );
    denied!(
        Method::Patch,
        "/admin/customer-groups/{id}",
        admin_rest::rename_group(
            &mut tx,
            &ctx,
            CustomerGroupId::new(),
            admin_rest::WriteGroup {
                name: String::new(),
                metadata: None
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/customer-groups/{id}",
        admin_rest::delete_group(&mut tx, &ctx, CustomerGroupId::new())
    );
    denied!(
        Method::Get,
        "/admin/customer-groups/{id}/customers",
        admin_rest::list_group_members(
            &mut tx,
            &ctx,
            CustomerGroupId::new(),
            admin_rest::List::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/customer-groups/{id}/customers",
        admin_rest::add_group_member(
            &mut tx,
            &ctx,
            CustomerGroupId::new(),
            admin_rest::GroupMember {
                customer_id: CustomerId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/customer-groups/{id}/customers/{customer_id}",
        admin_rest::remove_group_member(&mut tx, &ctx, CustomerGroupId::new(), CustomerId::new())
    );
    denied!(
        Method::Get,
        "/admin/promotions",
        admin_rest::list_promotions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/promotions",
        admin_rest::create_promotion(
            &mut tx,
            &ctx,
            admin_rest::CreatePromotion {
                code: String::new(),
                kind: tezgah::promotion::PromotionKind::Standard,
                status: tezgah::promotion::Status::Draft,
                is_automatic: false,
                campaign_id: None,
                usage_limit: None,
                customer_usage_limit: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/promotions/{id}",
        admin_rest::get_promotion(&mut tx, &ctx, PromotionId::new())
    );
    denied!(
        Method::Delete,
        "/admin/promotions/{id}",
        admin_rest::delete_promotion(&mut tx, &ctx, PromotionId::new())
    );
    denied!(
        Method::Patch,
        "/admin/promotions/{id}",
        admin_rest::update_promotion(
            &mut tx,
            &ctx,
            PromotionId::new(),
            admin_rest::UpdatePromotion::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/promotions/{id}/status",
        admin_rest::set_promotion_status(
            &mut tx,
            &ctx,
            PromotionId::new(),
            admin_rest::SetStatus {
                status: tezgah::promotion::Status::Draft
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/promotions/{id}/application-method",
        admin_rest::set_application_method(
            &mut tx,
            &ctx,
            PromotionId::new(),
            admin_rest::SetApplicationMethod {
                kind: tezgah::promotion::MethodKind::Fixed,
                target_type: tezgah::promotion::TargetKind::Order,
                allocation: None,
                value: Decimal::ZERO,
                currency_code: None,
                max_quantity: None,
                apply_to_quantity: None,
                buy_rules_min_quantity: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/promotions/{id}/{rule_type}",
        admin_rest::add_promotion_rule(
            &mut tx,
            &ctx,
            PromotionId::new(),
            "rules",
            admin_rest::AddRule {
                attribute: String::new(),
                operator: tezgah::promotion::Operator::Eq,
                allowed_values: vec![],
                description: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/promotions/{id}/{rule_type}",
        admin_rest::list_promotion_rules(&mut tx, &ctx, PromotionId::new(), "rules")
    );
    denied!(
        Method::Delete,
        "/admin/promotions/{id}/{rule_type}/{rule_id}",
        admin_rest::delete_promotion_rule(
            &mut tx,
            &ctx,
            PromotionId::new(),
            "rules",
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Get,
        "/admin/campaigns",
        admin_rest::list_campaigns(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/campaigns",
        admin_rest::create_campaign(
            &mut tx,
            &ctx,
            admin_rest::CreateCampaign {
                identifier: String::new(),
                name: String::new(),
                description: None,
                starts_at: None,
                ends_at: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/campaigns/{id}",
        admin_rest::get_campaign(&mut tx, &ctx, CampaignId::new())
    );
    denied!(
        Method::Patch,
        "/admin/campaigns/{id}",
        admin_rest::update_campaign(
            &mut tx,
            &ctx,
            CampaignId::new(),
            admin_rest::UpdateCampaign::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/campaigns/{id}/promotions",
        admin_rest::add_campaign_promotion(
            &mut tx,
            &ctx,
            CampaignId::new(),
            admin_rest::AttachPromotion {
                promotion_id: PromotionId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/campaigns/{id}/promotions/{promotion_id}",
        admin_rest::remove_campaign_promotion(&mut tx, &ctx, CampaignId::new(), PromotionId::new())
    );
    denied!(
        Method::Post,
        "/admin/campaigns/{id}/budget",
        admin_rest::set_campaign_budget(
            &mut tx,
            &ctx,
            CampaignId::new(),
            admin_rest::SetBudget {
                kind: tezgah::promotion::BudgetKind::Spend,
                cap: None,
                currency_code: None,
                attribute: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/campaigns/{id}/budget/usage",
        admin_rest::list_campaign_budget_usage(&mut tx, &ctx, CampaignId::new())
    );
    denied!(
        Method::Get,
        "/admin/tax-regions",
        admin_rest::list_tax_regions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/tax-regions",
        admin_rest::create_tax_region(
            &mut tx,
            &ctx,
            admin_rest::CreateTaxRegion {
                country_code: String::new(),
                province_code: None,
                parent_id: None,
                provider: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/tax-regions/{id}",
        admin_rest::get_tax_region(&mut tx, &ctx, TaxRegionId::new())
    );
    denied!(
        Method::Patch,
        "/admin/tax-regions/{id}",
        admin_rest::update_tax_region(
            &mut tx,
            &ctx,
            TaxRegionId::new(),
            admin_rest::UpdateTaxRegion::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/tax-regions/{id}",
        admin_rest::delete_tax_region(&mut tx, &ctx, TaxRegionId::new())
    );
    denied!(
        Method::Get,
        "/admin/tax-rates",
        admin_rest::list_tax_rates(&mut tx, &ctx, admin_rest::ListTaxRates::default())
    );
    denied!(
        Method::Post,
        "/admin/tax-rates",
        admin_rest::create_tax_rate(
            &mut tx,
            &ctx,
            admin_rest::CreateTaxRate {
                tax_region_id: TaxRegionId::new(),
                rate: Decimal::ZERO,
                code: None,
                name: String::new(),
                is_default: false,
                is_combinable: false,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/tax-rates/{id}",
        admin_rest::get_tax_rate(&mut tx, &ctx, TaxRateId::new())
    );
    denied!(
        Method::Patch,
        "/admin/tax-rates/{id}",
        admin_rest::update_tax_rate(
            &mut tx,
            &ctx,
            TaxRateId::new(),
            admin_rest::UpdateTaxRate::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/tax-rates/{id}",
        admin_rest::delete_tax_rate(&mut tx, &ctx, TaxRateId::new())
    );
    denied!(
        Method::Get,
        "/admin/tax-rates/{id}/rules",
        admin_rest::list_tax_rate_rules(&mut tx, &ctx, TaxRateId::new())
    );
    denied!(
        Method::Post,
        "/admin/tax-rates/{id}/rules",
        admin_rest::create_tax_rate_rule(
            &mut tx,
            &ctx,
            TaxRateId::new(),
            admin_rest::CreateTaxRateRule {
                reference: tezgah::tax::TaxReference::Product,
                reference_id: uuid::Uuid::now_v7(),
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/tax-rates/{id}/rules/{rule_id}",
        admin_rest::delete_tax_rate_rule(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Get,
        "/admin/regions",
        admin_rest::list_regions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/regions",
        admin_rest::create_region(
            &mut tx,
            &ctx,
            admin_rest::CreateRegion {
                name: String::new(),
                currency_code: "USD".to_string(),
                is_tax_inclusive: false,
                has_automatic_taxes: true,
                payment_providers: Vec::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/regions/{id}",
        admin_rest::get_region(&mut tx, &ctx, RegionId::new())
    );
    denied!(
        Method::Patch,
        "/admin/regions/{id}",
        admin_rest::update_region(
            &mut tx,
            &ctx,
            RegionId::new(),
            admin_rest::UpdateRegion::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/regions/{id}/countries",
        admin_rest::list_region_countries(
            &mut tx,
            &ctx,
            RegionId::new(),
            admin_rest::List::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/regions/{id}/countries",
        admin_rest::add_region_country(
            &mut tx,
            &ctx,
            RegionId::new(),
            admin_rest::AddRegionCountry {
                iso_2: String::new(),
                iso_3: String::new(),
                numeric_code: String::new(),
                name: String::new(),
                display_name: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/regions/{id}/countries/{country_code}",
        admin_rest::remove_region_country(&mut tx, &ctx, String::new())
    );
    denied!(
        Method::Get,
        "/admin/sales-channels",
        admin_rest::list_sales_channels(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/sales-channels",
        admin_rest::create_sales_channel(
            &mut tx,
            &ctx,
            admin_rest::CreateSalesChannel {
                name: String::new(),
                description: None,
                is_disabled: false
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/sales-channels/{id}",
        admin_rest::get_sales_channel(&mut tx, &ctx, SalesChannelId::new())
    );
    denied!(
        Method::Patch,
        "/admin/sales-channels/{id}",
        admin_rest::update_sales_channel(
            &mut tx,
            &ctx,
            SalesChannelId::new(),
            admin_rest::UpdateSalesChannel::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/sales-channels/{id}",
        admin_rest::delete_sales_channel(&mut tx, &ctx, SalesChannelId::new())
    );
    denied!(
        Method::Get,
        "/admin/publishable-api-keys",
        admin_rest::list_publishable_keys(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        Method::Post,
        "/admin/publishable-api-keys",
        admin_rest::create_publishable_key(
            &mut tx,
            &ctx,
            admin_rest::CreatePublishableKey {
                title: String::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/publishable-api-keys/{id}",
        admin_rest::get_publishable_key(&mut tx, &ctx, PublishableKeyId::new())
    );
    denied!(
        Method::Post,
        "/admin/publishable-api-keys/{id}/revoke",
        admin_rest::revoke_publishable_key(&mut tx, &ctx, PublishableKeyId::new())
    );
    denied!(
        Method::Get,
        "/admin/publishable-api-keys/{id}/sales-channels",
        admin_rest::list_key_sales_channels(&mut tx, &ctx, PublishableKeyId::new())
    );
    denied!(
        Method::Post,
        "/admin/publishable-api-keys/{id}/sales-channels",
        admin_rest::link_key_sales_channel(
            &mut tx,
            &ctx,
            PublishableKeyId::new(),
            admin_rest::LinkSalesChannel {
                sales_channel_id: SalesChannelId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/publishable-api-keys/{id}/sales-channels/{channel_id}",
        admin_rest::unlink_key_sales_channel(
            &mut tx,
            &ctx,
            PublishableKeyId::new(),
            SalesChannelId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/currencies",
        admin_rest::list_currencies(&mut tx, &ctx)
    );
    denied!(
        Method::Get,
        "/admin/currencies/{code}",
        admin_rest::get_currency(&mut tx, &ctx, "usd")
    );
    denied!(
        Method::Post,
        "/admin/currencies",
        admin_rest::create_currency(
            &mut tx,
            &ctx,
            admin_rest::CreateCurrency {
                code: "usd".to_string(),
                numeric_code: None,
                exponent: 2,
                symbol: "$".to_string(),
                symbol_native: "$".to_string(),
                name: "US dollar".to_string(),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/stores",
        admin_rest::get_store(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/admin/stores",
        admin_rest::create_store(
            &mut tx,
            &ctx,
            admin_rest::CreateStore {
                name: String::new(),
                default_currency_code: "USD".to_string(),
                supported_currency_codes: Vec::new(),
                supported_locales: Vec::new(),
                default_region_id: None,
                default_sales_channel_id: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Patch,
        "/admin/stores",
        admin_rest::update_store(&mut tx, &ctx, admin_rest::UpdateStore::default())
    );
    denied!(
        Method::Get,
        "/admin/locales",
        admin_rest::list_locales(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/admin/locales",
        admin_rest::set_locales(&mut tx, &ctx, admin_rest::SetLocales::default())
    );
    denied!(
        Method::Get,
        "/admin/workflows-executions",
        admin_rest::list_workflow_runs(&mut tx, &ctx, admin_rest::ListWorkflowRuns::default())
    );
    // The one handler that asks for itself: the runner takes no `Permit`, so
    // the route is where the question is put.
    denied!(
        Method::Get,
        "/admin/workflows-executions/{id}",
        admin_rest::get_workflow_run(&shop.pool, &ctx, WorkflowRunId::new())
    );
    denied!(
        Method::Get,
        "/admin/workflows-executions/{id}/steps",
        admin_rest::list_workflow_run_steps(&mut tx, &ctx, WorkflowRunId::new())
    );
    denied!(
        Method::Get,
        "/admin/workflow-dead-letters",
        admin_rest::list_workflow_dead_letters(&mut tx, &ctx, admin_rest::List::default())
    );

    // ------------------------------------------------ admin_catalogue.rs ---
    denied!(
        Method::Get,
        "/admin/products",
        admin_catalogue::list_products(&mut tx, &ctx, admin_catalogue::ListProducts::default())
    );
    denied!(
        Method::Post,
        "/admin/products",
        admin_catalogue::create_product(&mut tx, &ctx, admin_catalogue::CreateProduct::default())
    );
    denied!(
        Method::Post,
        "/admin/products/import",
        admin_catalogue::import_products(
            &mut tx,
            &ctx,
            admin_catalogue::ImportProductsBody::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/products/batch",
        admin_catalogue::batch_products(
            &mut tx,
            &ctx,
            admin_catalogue::ImportProductsBody::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/products/export",
        admin_catalogue::export_products(&mut tx, &ctx, admin_catalogue::ExportQuery::default())
    );
    denied!(
        Method::Get,
        "/admin/products/{id}",
        admin_catalogue::get_product(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Patch,
        "/admin/products/{id}",
        admin_catalogue::update_product(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::UpdateProduct::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}",
        admin_catalogue::delete_product(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/publish",
        admin_catalogue::publish_product(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/archive",
        admin_catalogue::archive_product(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/submit",
        admin_catalogue::submit_product_for_review(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/approve",
        admin_catalogue::approve_product(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/reject",
        admin_catalogue::reject_product(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::RejectProduct {
                reason: "not what was described".to_string()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/images",
        admin_catalogue::list_images(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/images",
        admin_catalogue::add_image(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::AddImage {
                url: "https://example.com/a.png".to_string(),
                alt_text: None,
                rank: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}/images/{image_id}",
        admin_catalogue::remove_image(&mut tx, &ctx, ProductImageId::new())
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/tags",
        admin_catalogue::list_product_tags(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/tags",
        admin_catalogue::tag_product(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::AttachTag {
                tag_id: ProductTagId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}/tags/{tag_id}",
        admin_catalogue::untag_product(&mut tx, &ctx, ProductId::new(), ProductTagId::new())
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/categories",
        admin_catalogue::list_product_categories(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/categories",
        admin_catalogue::add_product_to_category(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::AttachCategory {
                category_id: CategoryId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}/categories/{category_id}",
        admin_catalogue::remove_product_from_category(
            &mut tx,
            &ctx,
            ProductId::new(),
            CategoryId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/channels",
        admin_catalogue::list_product_channels(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/channels",
        admin_catalogue::add_product_to_channel(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::AttachChannel {
                sales_channel_id: SalesChannelId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}/channels/{sales_channel_id}",
        admin_catalogue::remove_product_from_channel(
            &mut tx,
            &ctx,
            ProductId::new(),
            SalesChannelId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/options",
        admin_catalogue::option_matrix(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/options",
        admin_catalogue::add_option(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::AddOption {
                title: "Size".to_string(),
                rank: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/product-options/{id}/values",
        admin_catalogue::add_option_value(
            &mut tx,
            &ctx,
            OptionId::new(),
            admin_catalogue::AddOptionValue {
                value: "Large".to_string(),
                rank: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/variants",
        admin_catalogue::list_variants(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/variants",
        admin_catalogue::create_variant(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::CreateVariant::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/variants/generate",
        admin_catalogue::generate_variants(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::GenerateVariants::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}",
        admin_catalogue::get_variant(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Patch,
        "/admin/product-variants/{id}",
        admin_catalogue::update_variant(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::UpdateVariant::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-variants/{id}",
        admin_catalogue::delete_variant(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}/options",
        admin_catalogue::variant_options(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/options",
        admin_catalogue::set_variant_options(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::SetVariantOptions { values: vec![] }
        )
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}/images",
        admin_catalogue::list_variant_images(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/images",
        admin_catalogue::attach_image_to_variant(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::AttachVariantImage {
                image_id: ProductImageId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-variants/{id}/images/{image_id}",
        admin_catalogue::detach_image_from_variant(
            &mut tx,
            &ctx,
            VariantId::new(),
            ProductImageId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/product-categories",
        admin_catalogue::list_categories(&mut tx, &ctx, admin_catalogue::ListCategories::default())
    );
    denied!(
        Method::Post,
        "/admin/product-categories",
        admin_catalogue::create_category(&mut tx, &ctx, admin_catalogue::CreateCategory::default())
    );
    denied!(
        Method::Get,
        "/admin/product-categories/{id}",
        admin_catalogue::get_category(&mut tx, &ctx, CategoryId::new())
    );
    denied!(
        Method::Patch,
        "/admin/product-categories/{id}",
        admin_catalogue::update_category(
            &mut tx,
            &ctx,
            CategoryId::new(),
            admin_catalogue::UpdateCategory::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-categories/{id}",
        admin_catalogue::delete_category(&mut tx, &ctx, CategoryId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-categories/{id}/move",
        admin_catalogue::move_category(
            &mut tx,
            &ctx,
            CategoryId::new(),
            admin_catalogue::MoveCategory::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/product-categories/{id}/subtree",
        admin_catalogue::category_subtree(
            &mut tx,
            &ctx,
            CategoryId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/product-tags",
        admin_catalogue::list_tags(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/product-tags",
        admin_catalogue::create_tag(
            &mut tx,
            &ctx,
            admin_catalogue::CreateValue {
                value: "sale".to_string(),
                external_id: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-tags/{id}",
        admin_catalogue::delete_tag(&mut tx, &ctx, ProductTagId::new())
    );
    denied!(
        Method::Get,
        "/admin/product-types",
        admin_catalogue::list_types(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/product-types",
        admin_catalogue::create_type(
            &mut tx,
            &ctx,
            admin_catalogue::CreateValue {
                value: "physical".to_string(),
                external_id: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-types/{id}",
        admin_catalogue::delete_type(&mut tx, &ctx, ProductTypeId::new())
    );
    denied!(
        Method::Get,
        "/admin/collections",
        admin_catalogue::list_collections(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/collections",
        admin_catalogue::create_collection(
            &mut tx,
            &ctx,
            admin_catalogue::CreateCollection {
                title: "Summer".to_string(),
                handle: "summer".to_string(),
                external_id: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/collections/{id}",
        admin_catalogue::get_collection(&mut tx, &ctx, CollectionId::new())
    );
    denied!(
        Method::Patch,
        "/admin/collections/{id}",
        admin_catalogue::update_collection(
            &mut tx,
            &ctx,
            CollectionId::new(),
            admin_catalogue::UpdateCollection::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/collections/{id}",
        admin_catalogue::delete_collection(&mut tx, &ctx, CollectionId::new())
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/translations",
        admin_catalogue::list_translations(&mut tx, &ctx, ProductId::new())
    );
    denied!(
        Method::Post,
        "/admin/products/{id}/translations",
        admin_catalogue::put_translation(
            &mut tx,
            &ctx,
            ProductId::new(),
            admin_catalogue::PutTranslation {
                locale: "tr".to_string(),
                title: "Başlık".to_string(),
                subtitle: None,
                description: None,
                handle: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/products/{id}/translations/{locale}",
        admin_catalogue::localised(&mut tx, &ctx, ProductId::new(), "tr")
    );
    denied!(
        Method::Delete,
        "/admin/products/{id}/translations/{locale}",
        admin_catalogue::remove_translation(&mut tx, &ctx, ProductId::new(), "tr")
    );
    denied!(
        Method::Get,
        "/admin/product-categories/{id}/translations",
        admin_catalogue::list_category_translations(&mut tx, &ctx, CategoryId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-categories/{id}/translations",
        admin_catalogue::put_category_translation(
            &mut tx,
            &ctx,
            CategoryId::new(),
            admin_catalogue::PutCategoryTranslation {
                locale: "tr".to_string(),
                name: "Başlık".to_string(),
                description: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/product-categories/{id}/translations/{locale}",
        admin_catalogue::localised_category(&mut tx, &ctx, CategoryId::new(), "tr")
    );
    denied!(
        Method::Delete,
        "/admin/product-categories/{id}/translations/{locale}",
        admin_catalogue::remove_category_translation(&mut tx, &ctx, CategoryId::new(), "tr")
    );
    denied!(
        Method::Post,
        "/admin/price-sets",
        admin_catalogue::create_price_set(&mut tx, &ctx)
    );
    denied!(
        Method::Get,
        "/admin/price-sets/{id}",
        admin_catalogue::get_price_set(&mut tx, &ctx, PriceSetId::new())
    );
    denied!(
        Method::Get,
        "/admin/price-sets/{id}/prices",
        admin_catalogue::list_prices(
            &mut tx,
            &ctx,
            PriceSetId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/price-set",
        admin_catalogue::link_variant_price_set(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::LinkPriceSet {
                price_set_id: PriceSetId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/bundle",
        admin_catalogue::set_bundle_price(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::SetBundlePrice {
                mode: None,
                discount_percent: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}/bundle/components",
        admin_catalogue::list_bundle_components(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/bundle/components",
        admin_catalogue::add_bundle_component(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::NewBundleComponentInput {
                component_variant_id: VariantId::new(),
                quantity: 1,
                sort_order: 0,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-variants/{id}/bundle/components/{component_variant_id}",
        admin_catalogue::remove_bundle_component(&mut tx, &ctx, VariantId::new(), VariantId::new())
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}/bundle/price",
        admin_catalogue::bundle_price(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::BundlePriceQuery {
                currency_code: "USD".to_string(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/prices",
        admin_catalogue::add_price(
            &mut tx,
            &ctx,
            admin_catalogue::AddPrice {
                price_set_id: PriceSetId::new(),
                price_list_id: None,
                title: None,
                amount: Decimal::ZERO,
                currency_code: "USD".to_string(),
                min_quantity: None,
                max_quantity: None,
                rules: vec![],
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/prices/batch",
        admin_catalogue::batch_prices(&mut tx, &ctx, admin_catalogue::UpdatePricesBody::default())
    );
    denied!(
        Method::Patch,
        "/admin/prices/{id}",
        admin_catalogue::update_price(
            &mut tx,
            &ctx,
            PriceId::new(),
            admin_catalogue::UpdatePrice::default()
        )
    );
    denied!(
        Method::Delete,
        "/admin/prices/{id}",
        admin_catalogue::delete_price(&mut tx, &ctx, PriceId::new())
    );
    denied!(
        Method::Get,
        "/admin/prices/{id}/rules",
        admin_catalogue::list_price_rules(&mut tx, &ctx, PriceId::new())
    );
    denied!(
        Method::Post,
        "/admin/prices/{id}/rules",
        admin_catalogue::add_price_rule(
            &mut tx,
            &ctx,
            PriceId::new(),
            admin_catalogue::PriceRuleInput {
                attribute: "region_id".to_string(),
                value: "reg_1".to_string(),
                operator: "eq".to_string(),
                priority: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/prices/{id}/rules/{rule_id}",
        admin_catalogue::remove_price_rule(&mut tx, &ctx, PriceId::new(), uuid::Uuid::now_v7())
    );
    denied!(
        Method::Get,
        "/admin/price-lists",
        admin_catalogue::list_price_lists(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/price-lists",
        admin_catalogue::create_price_list(
            &mut tx,
            &ctx,
            admin_catalogue::CreatePriceList {
                title: "Sale".to_string(),
                description: None,
                kind: "sale".to_string(),
                status: "active".to_string(),
                starts_at: None,
                ends_at: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/price-lists/{id}",
        admin_catalogue::get_price_list(&mut tx, &ctx, PriceListId::new())
    );
    denied!(
        Method::Patch,
        "/admin/price-lists/{id}",
        admin_catalogue::update_price_list(
            &mut tx,
            &ctx,
            PriceListId::new(),
            admin_catalogue::UpdatePriceList::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/price-lists/{id}/rules",
        admin_catalogue::add_price_list_rule(
            &mut tx,
            &ctx,
            PriceListId::new(),
            admin_catalogue::AddPriceListRule {
                attribute: "region_id".to_string(),
                allowed_values: vec![],
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/price-preferences",
        admin_catalogue::get_price_preference(
            &mut tx,
            &ctx,
            admin_catalogue::FindPricePreference::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/price-preferences",
        admin_catalogue::set_price_preference(
            &mut tx,
            &ctx,
            admin_catalogue::SetPricePreference {
                attribute: "region_id".to_string(),
                value: None,
                is_tax_inclusive: false,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/stock-locations",
        admin_catalogue::list_stock_locations(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/stock-locations",
        admin_catalogue::create_stock_location(
            &mut tx,
            &ctx,
            admin_catalogue::CreateStockLocation {
                name: "Main".to_string(),
                address: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/stock-locations/{id}",
        admin_catalogue::get_stock_location(&mut tx, &ctx, StockLocationId::new())
    );
    denied!(
        Method::Patch,
        "/admin/stock-locations/{id}",
        admin_catalogue::rename_stock_location(
            &mut tx,
            &ctx,
            StockLocationId::new(),
            admin_catalogue::RenameStockLocation {
                name: "New".to_string()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/stock-locations/{id}",
        admin_catalogue::delete_stock_location(&mut tx, &ctx, StockLocationId::new())
    );
    denied!(
        Method::Get,
        "/admin/stock-locations/{id}/address",
        admin_catalogue::get_stock_location_address(&mut tx, &ctx, StockLocationId::new())
    );
    denied!(
        Method::Post,
        "/admin/stock-locations/{id}/address",
        admin_catalogue::set_stock_location_address(
            &mut tx,
            &ctx,
            StockLocationId::new(),
            admin_catalogue::StockLocationAddressIn {
                address_1: "1 Main St".to_string(),
                address_2: None,
                company: None,
                city: None,
                country_code: "US".to_string(),
                province: None,
                postal_code: None,
                phone: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/stock-locations/{id}/sales-channels",
        admin_catalogue::link_sales_channel(
            &mut tx,
            &ctx,
            StockLocationId::new(),
            admin_catalogue::LinkSalesChannel {
                sales_channel_id: SalesChannelId::new()
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/stock-locations/{id}/sales-channels/{sales_channel_id}",
        admin_catalogue::unlink_sales_channel(
            &mut tx,
            &ctx,
            StockLocationId::new(),
            SalesChannelId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/sales-channels/{id}/stock-locations",
        admin_catalogue::list_locations_for_sales_channel(
            &mut tx,
            &ctx,
            SalesChannelId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-items",
        admin_catalogue::list_inventory_items(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/inventory-items",
        admin_catalogue::create_inventory_item(
            &mut tx,
            &ctx,
            admin_catalogue::CreateInventoryItem {
                sku: None,
                title: None,
                requires_shipping: true,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-items/batch",
        admin_catalogue::batch_stock_levels(
            &mut tx,
            &ctx,
            admin_catalogue::SetStockLevelsBody::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-items/{id}",
        admin_catalogue::get_inventory_item(&mut tx, &ctx, InventoryItemId::new())
    );
    denied!(
        Method::Delete,
        "/admin/inventory-items/{id}",
        admin_catalogue::delete_inventory_item(&mut tx, &ctx, InventoryItemId::new())
    );
    denied!(
        Method::Get,
        "/admin/inventory-items/{id}/location-levels",
        admin_catalogue::list_levels(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-items/{id}/location-levels",
        admin_catalogue::set_stock(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            admin_catalogue::SetStock {
                location_id: StockLocationId::new(),
                stocked_quantity: 0,
                incoming_quantity: 0,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-items/{id}/location-levels/{location_id}",
        admin_catalogue::get_level(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            StockLocationId::new()
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-items/{id}/location-levels/{location_id}/adjust",
        admin_catalogue::adjust_stock(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            StockLocationId::new(),
            admin_catalogue::AdjustStock {
                delta: 1,
                reason: None
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-items/{id}/transfers",
        admin_catalogue::transfer_stock(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            admin_catalogue::TransferStock {
                from_location_id: StockLocationId::new(),
                to_location_id: StockLocationId::new(),
                quantity: 1,
                reason: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-items/{id}/transfers",
        admin_catalogue::list_stock_transfers(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            admin_catalogue::ListQuery::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/stock-transfers/{id}",
        admin_catalogue::get_stock_transfer(&mut tx, &ctx, StockTransferId::new())
    );
    denied!(
        Method::Get,
        "/admin/product-variants/{id}/inventory-items",
        admin_catalogue::list_variant_inventory_items(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Post,
        "/admin/product-variants/{id}/inventory-items",
        admin_catalogue::attach_inventory_item(
            &mut tx,
            &ctx,
            VariantId::new(),
            admin_catalogue::AttachInventoryItem {
                inventory_item_id: InventoryItemId::new(),
                required_quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/product-variants/{id}/inventory-items/{inventory_item_id}",
        admin_catalogue::detach_inventory_item(
            &mut tx,
            &ctx,
            VariantId::new(),
            InventoryItemId::new()
        )
    );
    denied!(
        Method::Get,
        "/admin/reservations",
        admin_catalogue::list_reservations(&mut tx, &ctx, admin_catalogue::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/reservations",
        admin_catalogue::create_reservation(
            &mut tx,
            &ctx,
            admin_catalogue::CreateReservation {
                inventory_item_id: InventoryItemId::new(),
                location_id: StockLocationId::new(),
                quantity: 1,
                line_item_id: None,
                allows_backorder: false,
                expires_at: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/reservations/{id}",
        admin_catalogue::release_reservation(&mut tx, &ctx, ReservationId::new())
    );
    denied!(
        Method::Post,
        "/admin/reservations/{id}/fulfil",
        admin_catalogue::fulfil_reservation(&mut tx, &ctx, ReservationId::new())
    );

    // ----------------------------------------------------- admin_order.rs --
    denied!(
        Method::Get,
        "/admin/orders",
        admin_order::list_orders(&mut tx, &ctx, admin_order::ListOrders::default())
    );
    denied!(
        Method::Post,
        "/admin/orders",
        admin_order::create_order(
            &mut tx,
            &ctx,
            admin_order::CreateOrder {
                currency: "TRY".into(),
                email: None,
                customer_id: None,
                region_id: None,
                sales_channel_id: None,
                locale: None,
                lines: vec![],
                shipping: vec![],
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/line-items",
        admin_order::order_line_items(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/ledger",
        admin_order::order_ledger(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/transactions",
        admin_order::order_transactions(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/changes",
        admin_order::order_changes(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::Listing::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/returns",
        admin_order::order_returns(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::Listing::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/fulfillments",
        admin_order::order_fulfillments(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::Listing::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/fulfillments",
        admin_order::create_fulfillment(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::CreateFulfillment {
                location_id: StockLocationId::new(),
                shipping_option_id: None,
                provider_id: None,
                requires_shipping: true,
                data: None,
                items: vec![],
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/draft-orders",
        admin_order::list_draft_orders(&mut tx, &ctx, admin_order::ListOrders::default())
    );
    denied!(
        Method::Post,
        "/admin/draft-orders",
        admin_order::create_draft_order(
            &mut tx,
            &ctx,
            admin_order::CreateOrder {
                currency: "TRY".into(),
                email: None,
                customer_id: None,
                region_id: None,
                sales_channel_id: None,
                locale: None,
                lines: vec![],
                shipping: vec![],
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/order-edits",
        admin_order::list_order_edits(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::Listing::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/order-edits/{id}",
        admin_order::get_order_edit(&mut tx, &ctx, OrderChangeId::new())
    );
    denied!(
        Method::Get,
        "/admin/order-changes/{id}",
        admin_order::get_order_change(&mut tx, &ctx, OrderChangeId::new())
    );
    denied!(
        Method::Get,
        "/admin/returns",
        admin_order::list_returns(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Get,
        "/admin/returns/{id}/items",
        admin_order::return_items(&mut tx, &ctx, ReturnId::new())
    );
    denied!(
        Method::Get,
        "/admin/exchanges",
        admin_order::list_exchanges(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Get,
        "/admin/claims",
        admin_order::list_claims(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Get,
        "/admin/payments",
        admin_order::list_payments(&mut tx, &ctx, admin_order::ListPayments::default())
    );
    // A provider's callback asks before it writes anything down, which is
    // what makes it deniable at all: `record_webhook`'s permit is its first
    // statement, ahead of the provider lookup.
    denied!(
        Method::Post,
        "/webhooks/payments/{provider}",
        admin_order::receive_callback(
            &mut tx,
            &ctx,
            "demo-bank",
            admin_order::ProviderCallback {
                event_id: "evt_denied".into(),
                event_type: "payment_intent.succeeded".into(),
                kind: tezgah::payment::WebhookKind::Authorized,
                session_id: None,
                amount: None,
                payload: serde_json::json!({}),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/payment-webhooks",
        admin_order::pending_callbacks(&mut tx, &ctx, admin_order::ListCallbacks::default())
    );
    denied!(
        Method::Post,
        "/admin/payment-webhooks/{id}/apply",
        admin_order::apply_callback(&mut tx, &ctx, PaymentWebhookEventId::new())
    );
    denied!(
        Method::Post,
        "/admin/payment-webhooks/{id}/processed",
        admin_order::callback_processed(&mut tx, &ctx, PaymentWebhookEventId::new())
    );
    denied!(
        Method::Get,
        "/admin/payments/{id}",
        admin_order::get_payment(&mut tx, &ctx, PaymentId::new())
    );
    denied!(
        Method::Post,
        "/admin/payments/{id}/capture",
        admin_order::capture_payment(
            &mut tx,
            &ctx,
            PaymentId::new(),
            admin_order::CapturePayment {
                amount: try_(dec!(0)),
                metadata: None
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/payments/{id}/refund",
        admin_order::refund_payment(
            &mut tx,
            &ctx,
            PaymentId::new(),
            admin_order::RefundPayment {
                amount: try_(dec!(0)),
                reason_id: None,
                note: None
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/refund-to-credit",
        admin_order::refund_order_to_credit(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::RefundToCredit {
                amount: try_(dec!(0)),
                reason: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/payments/payment-providers",
        admin_order::payment_providers(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/admin/payments/payment-providers",
        admin_order::register_payment_provider(
            &mut tx,
            &ctx,
            admin_order::RegisterPaymentProvider { code: "x".into() }
        )
    );
    denied!(
        Method::Post,
        "/admin/payments/payment-providers/{id}/disable",
        admin_order::disable_payment_provider(&mut tx, &ctx, PaymentProviderId::new())
    );
    denied!(
        Method::Post,
        "/admin/payments/payment-providers/{id}/enable",
        admin_order::enable_payment_provider(&mut tx, &ctx, PaymentProviderId::new())
    );
    denied!(
        Method::Post,
        "/admin/payment-collections",
        admin_order::create_payment_collection(
            &mut tx,
            &ctx,
            admin_order::CreateCollection {
                amount: try_(dec!(0)),
                metadata: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/payment-collections/{id}",
        admin_order::get_payment_collection(&mut tx, &ctx, PaymentCollectionId::new())
    );
    denied!(
        Method::Get,
        "/admin/payment-collections/{id}/payment-sessions",
        admin_order::payment_sessions(
            &mut tx,
            &ctx,
            PaymentCollectionId::new(),
            admin_order::Listing::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/payment-collections/{id}/payment-sessions",
        admin_order::create_payment_session(
            &mut tx,
            &ctx,
            PaymentCollectionId::new(),
            admin_order::CreateSession {
                provider_code: "fake".into(),
                amount: try_(dec!(0)),
                context: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/refund-reasons",
        admin_order::list_refund_reasons(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/refund-reasons",
        admin_order::create_refund_reason(
            &mut tx,
            &ctx,
            admin_order::NewReason {
                code: "x".into(),
                label: "x".into(),
                description: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/return-reasons",
        admin_order::list_return_reasons(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/return-reasons",
        admin_order::create_return_reason(
            &mut tx,
            &ctx,
            admin_order::NewReason {
                code: "x".into(),
                label: "x".into(),
                description: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/return-reasons/{id}/translations",
        admin_order::list_return_reason_translations(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/return-reasons/{id}/translations",
        admin_order::put_return_reason_translation(
            &mut tx,
            &ctx,
            uuid::Uuid::now_v7(),
            admin_order::PutReturnReasonTranslation {
                locale: "tr".to_string(),
                label: "İade".to_string(),
                description: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/return-reasons/{id}/translations/{locale}",
        admin_order::localised_return_reason(&mut tx, &ctx, uuid::Uuid::now_v7(), "tr")
    );
    denied!(
        Method::Delete,
        "/admin/return-reasons/{id}/translations/{locale}",
        admin_order::remove_return_reason_translation(&mut tx, &ctx, uuid::Uuid::now_v7(), "tr")
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}",
        admin_order::get_fulfillment(&mut tx, &ctx, OrderId::new(), FulfillmentId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/pack",
        admin_order::pack_fulfillment(&mut tx, &ctx, OrderId::new(), FulfillmentId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/shipment",
        admin_order::ship_fulfillment(
            &mut tx,
            &ctx,
            OrderId::new(),
            FulfillmentId::new(),
            admin_order::ShipFulfillment { labels: vec![] }
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/mark-as-delivered",
        admin_order::deliver_fulfillment(&mut tx, &ctx, OrderId::new(), FulfillmentId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/cancel",
        admin_order::cancel_fulfillment(&mut tx, &ctx, OrderId::new(), FulfillmentId::new())
    );
    denied!(
        Method::Get,
        "/admin/fulfillment-sets",
        admin_order::list_fulfillment_sets(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/fulfillment-sets",
        admin_order::create_fulfillment_set(
            &mut tx,
            &ctx,
            admin_order::CreateFulfillmentSet {
                name: "x".into(),
                kind: admin_order::SetKindIn::Shipping
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/fulfillment-sets/{id}",
        admin_order::delete_fulfillment_set(&mut tx, &ctx, FulfillmentSetId::new())
    );
    denied!(
        Method::Get,
        "/admin/fulfillment-sets/{id}/service-zones",
        admin_order::service_zones(&mut tx, &ctx, FulfillmentSetId::new())
    );
    denied!(
        Method::Post,
        "/admin/fulfillment-sets/{id}/service-zones",
        admin_order::create_service_zone(
            &mut tx,
            &ctx,
            FulfillmentSetId::new(),
            admin_order::CreateServiceZone { name: "x".into() }
        )
    );
    denied!(
        Method::Get,
        "/admin/fulfillment-providers",
        admin_order::fulfillment_providers(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/admin/fulfillment-providers",
        admin_order::register_fulfillment_provider(
            &mut tx,
            &ctx,
            admin_order::RegisterProvider { name: "x".into() }
        )
    );
    denied!(
        Method::Post,
        "/admin/fulfillment-providers/{id}/disable",
        admin_order::disable_fulfillment_provider(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/fulfillment-providers/{id}/enable",
        admin_order::enable_fulfillment_provider(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Get,
        "/admin/shipping-options",
        admin_order::list_shipping_options(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/shipping-options",
        admin_order::create_shipping_option(
            &mut tx,
            &ctx,
            admin_order::CreateShippingOption {
                name: "x".into(),
                price_type: admin_order::PriceKindIn::Flat,
                service_zone_id: ServiceZoneId::new(),
                shipping_profile_id: None,
                provider_id: None,
                shipping_option_type_id: None,
                data: None,
                is_return: false,
                enabled_in_store: true,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-options/{id}",
        admin_order::get_shipping_option(&mut tx, &ctx, ShippingOptionId::new())
    );
    denied!(
        Method::Patch,
        "/admin/shipping-options/{id}",
        admin_order::update_shipping_option(
            &mut tx,
            &ctx,
            ShippingOptionId::new(),
            admin_order::UpdateShippingOption::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/shipping-options/{id}/rules",
        admin_order::create_shipping_option_rule(
            &mut tx,
            &ctx,
            ShippingOptionId::new(),
            admin_order::CreateShippingOptionRule {
                attribute: "x".into(),
                operator: "eq".into(),
                value: serde_json::json!(null),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-options/{id}/translations",
        admin_order::list_shipping_option_translations(&mut tx, &ctx, ShippingOptionId::new())
    );
    denied!(
        Method::Post,
        "/admin/shipping-options/{id}/translations",
        admin_order::put_shipping_option_translation(
            &mut tx,
            &ctx,
            ShippingOptionId::new(),
            admin_order::PutShippingOptionTranslation {
                locale: "tr".to_string(),
                name: "Standart teslimat".to_string(),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-options/{id}/translations/{locale}",
        admin_order::localised_shipping_option(&mut tx, &ctx, ShippingOptionId::new(), "tr")
    );
    denied!(
        Method::Delete,
        "/admin/shipping-options/{id}/translations/{locale}",
        admin_order::remove_shipping_option_translation(
            &mut tx,
            &ctx,
            ShippingOptionId::new(),
            "tr"
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-profiles",
        admin_order::list_shipping_profiles(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/shipping-profiles",
        admin_order::create_shipping_profile(
            &mut tx,
            &ctx,
            admin_order::CreateShippingProfile {
                name: "x".into(),
                kind: "x".into()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-profiles/{id}",
        admin_order::get_shipping_profile(&mut tx, &ctx, ShippingProfileId::new())
    );
    denied!(
        Method::Patch,
        "/admin/shipping-profiles/{id}",
        admin_order::update_shipping_profile(
            &mut tx,
            &ctx,
            ShippingProfileId::new(),
            admin_order::UpdateShippingProfile::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/shipping-option-types",
        admin_order::list_shipping_option_types(&mut tx, &ctx, admin_order::Listing::default())
    );
    denied!(
        Method::Post,
        "/admin/shipping-option-types",
        admin_order::create_shipping_option_type(
            &mut tx,
            &ctx,
            admin_order::CreateShippingOptionType {
                label: "x".into(),
                code: "x".into(),
                description: None
            }
        )
    );

    // Orders, draft orders, edits, returns, exchanges and claims: each of
    // these used to be TOLERATED as loading its row before permit. See
    // productdevbook/tezgah#151 and productdevbook/tezgah#152.
    denied!(
        Method::Get,
        "/admin/orders/{id}",
        admin_order::get_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/complete",
        admin_order::complete_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/cancel",
        admin_order::cancel_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/archive",
        admin_order::archive_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Patch,
        "/admin/orders/{id}/shipping-address",
        admin_order::update_order_shipping_address(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::AddressIn::default()
        )
    );
    denied!(
        Method::Patch,
        "/admin/orders/{id}/billing-address",
        admin_order::update_order_billing_address(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::AddressIn::default()
        )
    );
    denied!(
        Method::Patch,
        "/admin/orders/{id}/email",
        admin_order::update_order_email(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::UpdateEmail {
                email: "shopper@example.com".into(),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/items",
        admin_order::order_items(&mut tx, &ctx, OrderId::new(), None)
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/shipping-methods",
        admin_order::order_shipping_methods(&mut tx, &ctx, OrderId::new(), None)
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/summary",
        admin_order::order_summary(&mut tx, &ctx, OrderId::new(), None)
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/totals",
        admin_order::order_totals(&mut tx, &ctx, OrderId::new(), None)
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/shipping-options",
        admin_order::order_shipping_options(&mut tx, &ctx, OrderId::new(), "TR")
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/returns/shipping-options",
        admin_order::return_shipping_options(&mut tx, &ctx, OrderId::new(), "TR")
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/transactions",
        admin_order::record_transaction(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::RecordTransaction {
                amount: try_(dec!(0)),
                reference: "manual".into(),
                reference_id: uuid::Uuid::now_v7(),
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/payment-collection",
        admin_order::attach_order_payment_collection(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::AttachPaymentCollection {
                payment_collection_id: PaymentCollectionId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/order-edits",
        admin_order::open_order_edit(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::OpenEdit { description: None }
        )
    );
    denied!(
        Method::Post,
        "/admin/draft-orders/{id}/edit",
        admin_order::open_draft_edit(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::OpenEdit { description: None }
        )
    );
    denied!(
        Method::Get,
        "/admin/draft-orders/{id}",
        admin_order::get_draft_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Delete,
        "/admin/draft-orders/{id}",
        admin_order::cancel_draft_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/draft-orders/{id}/convert-to-order",
        admin_order::convert_draft_order(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::ConvertDraft {
                payment_collection_id: PaymentCollectionId::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/draft-orders/{id}/edit",
        admin_order::get_draft_edit(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Delete,
        "/admin/draft-orders/{id}/edit",
        admin_order::decline_draft_edit(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::DeclineChange { reason: None }
        )
    );
    denied!(
        Method::Post,
        "/admin/draft-orders/{id}/edit/items",
        admin_order::add_draft_edit_item(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::AddItemAction {
                action: admin_order::ItemAction::Add,
                order_line_item_id: LineItemId::new(),
                quantity: 1,
                unit_price: None,
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/draft-orders/{id}/edit/items/{action_id}",
        admin_order::remove_draft_edit_item(&mut tx, &ctx, OrderId::new(), uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/draft-orders/{id}/edit/shipping-methods",
        admin_order::add_draft_edit_shipping(
            &mut tx,
            &ctx,
            OrderId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/draft-orders/{id}/edit/shipping-methods/{action_id}",
        admin_order::remove_draft_edit_shipping(
            &mut tx,
            &ctx,
            OrderId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/draft-orders/{id}/edit/confirm",
        admin_order::confirm_draft_edit(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Delete,
        "/admin/order-edits/{id}",
        admin_order::decline_order_edit(
            &mut tx,
            &ctx,
            OrderChangeId::new(),
            admin_order::DeclineChange { reason: None }
        )
    );
    denied!(
        Method::Post,
        "/admin/order-edits/{id}/items",
        admin_order::add_order_edit_item(
            &mut tx,
            &ctx,
            OrderChangeId::new(),
            admin_order::AddItemAction {
                action: admin_order::ItemAction::Add,
                order_line_item_id: LineItemId::new(),
                quantity: 1,
                unit_price: None,
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/order-edits/{id}/items/{action_id}",
        admin_order::remove_order_edit_item(
            &mut tx,
            &ctx,
            OrderChangeId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/order-edits/{id}/shipping-method",
        admin_order::add_order_edit_shipping(
            &mut tx,
            &ctx,
            OrderChangeId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/order-edits/{id}/shipping-method/{action_id}",
        admin_order::remove_order_edit_shipping(
            &mut tx,
            &ctx,
            OrderChangeId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/order-edits/{id}/confirm",
        admin_order::confirm_order_edit(&mut tx, &ctx, OrderChangeId::new())
    );
    denied!(
        Method::Post,
        "/admin/returns",
        admin_order::request_return(
            &mut tx,
            &ctx,
            admin_order::RequestReturn {
                order_id: OrderId::new(),
                location_id: None,
                lines: vec![],
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/returns/{id}",
        admin_order::get_return(&mut tx, &ctx, ReturnId::new())
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/cancel",
        admin_order::cancel_return(&mut tx, &ctx, ReturnId::new())
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/receive",
        admin_order::receive_return(
            &mut tx,
            &ctx,
            ReturnId::new(),
            admin_order::ReceiveReturn { lines: vec![] }
        )
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/dismiss-items",
        admin_order::dismiss_return_items(
            &mut tx,
            &ctx,
            ReturnId::new(),
            admin_order::ReceiveReturn { lines: vec![] }
        )
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/request-items",
        admin_order::add_return_request_item(
            &mut tx,
            &ctx,
            ReturnId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/returns/{id}/request-items/{action_id}",
        admin_order::remove_return_request_item(
            &mut tx,
            &ctx,
            ReturnId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/receive-items",
        admin_order::add_return_receive_item(
            &mut tx,
            &ctx,
            ReturnId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/returns/{id}/receive-items/{action_id}",
        admin_order::remove_return_receive_item(
            &mut tx,
            &ctx,
            ReturnId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/shipping-method",
        admin_order::add_return_shipping(
            &mut tx,
            &ctx,
            ReturnId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/returns/{id}/shipping-method/{action_id}",
        admin_order::remove_return_shipping(&mut tx, &ctx, ReturnId::new(), uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/request",
        admin_order::confirm_return_request(&mut tx, &ctx, ReturnId::new())
    );
    denied!(
        Method::Post,
        "/admin/exchanges",
        admin_order::request_exchange(
            &mut tx,
            &ctx,
            admin_order::RequestExchange {
                order_id: OrderId::new(),
                returning: vec![],
                outbound: vec![],
                location_id: None,
                allow_backorder: false,
                difference_due: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/exchanges/{id}",
        admin_order::get_exchange(&mut tx, &ctx, ExchangeId::new())
    );
    denied!(
        Method::Get,
        "/admin/exchanges/{id}/items",
        admin_order::exchange_actions(&mut tx, &ctx, ExchangeId::new())
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/cancel",
        admin_order::cancel_exchange(&mut tx, &ctx, ExchangeId::new())
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/inbound/items",
        admin_order::add_exchange_inbound_item(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/exchanges/{id}/inbound/items/{action_id}",
        admin_order::remove_exchange_inbound_item(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/inbound/shipping-method",
        admin_order::add_exchange_inbound_shipping(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/outbound/items",
        admin_order::add_exchange_outbound_item(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/exchanges/{id}/outbound/items/{action_id}",
        admin_order::remove_exchange_outbound_item(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/outbound/shipping-method",
        admin_order::add_exchange_outbound_shipping(
            &mut tx,
            &ctx,
            ExchangeId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/exchanges/{id}/request",
        admin_order::confirm_exchange_request(&mut tx, &ctx, ExchangeId::new())
    );
    denied!(
        Method::Post,
        "/admin/claims",
        admin_order::request_claim(
            &mut tx,
            &ctx,
            admin_order::RequestClaim {
                order_id: OrderId::new(),
                claim_type: admin_order::ClaimKind::Refund,
                faulty: vec![],
                replacements: vec![],
                collect: false,
                location_id: None,
                refund_amount: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/claims/{id}",
        admin_order::get_claim(&mut tx, &ctx, ClaimId::new())
    );
    denied!(
        Method::Get,
        "/admin/claims/{id}/lines",
        admin_order::claim_lines(&mut tx, &ctx, ClaimId::new())
    );
    denied!(
        Method::Get,
        "/admin/claims/{id}/items",
        admin_order::claim_actions(&mut tx, &ctx, ClaimId::new())
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/cancel",
        admin_order::cancel_claim(&mut tx, &ctx, ClaimId::new())
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/claim-items",
        admin_order::add_claim_item(
            &mut tx,
            &ctx,
            ClaimId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/claims/{id}/claim-items/{action_id}",
        admin_order::remove_claim_item(&mut tx, &ctx, ClaimId::new(), uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/inbound/items",
        admin_order::add_claim_inbound_item(
            &mut tx,
            &ctx,
            ClaimId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/claims/{id}/inbound/items/{action_id}",
        admin_order::remove_claim_inbound_item(&mut tx, &ctx, ClaimId::new(), uuid::Uuid::now_v7())
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/inbound/shipping-method",
        admin_order::add_claim_inbound_shipping(
            &mut tx,
            &ctx,
            ClaimId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/outbound/items",
        admin_order::add_claim_outbound_item(
            &mut tx,
            &ctx,
            ClaimId::new(),
            admin_order::LineQuantity {
                order_line_item_id: LineItemId::new(),
                quantity: 1,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/claims/{id}/outbound/items/{action_id}",
        admin_order::remove_claim_outbound_item(
            &mut tx,
            &ctx,
            ClaimId::new(),
            uuid::Uuid::now_v7()
        )
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/outbound/shipping-method",
        admin_order::add_claim_outbound_shipping(
            &mut tx,
            &ctx,
            ClaimId::new(),
            admin_order::AddShippingAction {
                name: "x".into(),
                amount: try_(dec!(0)),
                internal_note: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/claims/{id}/request",
        admin_order::confirm_claim_request(&mut tx, &ctx, ClaimId::new())
    );

    // -------------------------------------------------------- store.rs -----
    denied!(
        Method::Get,
        "/store/products",
        store::list_products(&mut tx, &ctx, "test-token", store::ListProducts::default())
    );
    denied!(
        Method::Get,
        "/store/products/{handle}",
        store::get_product(&mut tx, &ctx, "test-token", "example-handle", None)
    );
    denied!(
        Method::Get,
        "/store/product-variants",
        store::list_variants(
            &mut tx,
            &ctx,
            "test-token",
            store::ListVariants {
                product_id: ProductId::new(),
                after: None,
                limit: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/store/product-variants/{id}",
        store::get_variant(&mut tx, &ctx, "test-token", VariantId::new())
    );
    denied!(
        Method::Get,
        "/store/product-options",
        store::list_product_options(
            &mut tx,
            &ctx,
            "test-token",
            store::ListOptions {
                product_id: ProductId::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/store/product-options/{id}",
        store::get_product_option(&mut tx, &ctx, "test-token", OptionId::new())
    );
    denied!(
        Method::Get,
        "/store/product-tags",
        store::list_product_tags(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Get,
        "/store/product-tags/{id}",
        store::get_product_tag(&mut tx, &ctx, ProductTagId::new())
    );
    denied!(
        Method::Get,
        "/store/product-types",
        store::list_product_types(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Get,
        "/store/product-types/{id}",
        store::get_product_type(&mut tx, &ctx, ProductTypeId::new())
    );
    denied!(
        Method::Get,
        "/store/product-categories",
        store::list_product_categories(&mut tx, &ctx, store::ListCategories::default())
    );
    denied!(
        Method::Get,
        "/store/product-categories/{id}",
        store::get_product_category(&mut tx, &ctx, CategoryId::new(), None)
    );
    denied!(
        Method::Get,
        "/store/collections",
        store::list_collections(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Get,
        "/store/collections/{id}",
        store::get_collection(&mut tx, &ctx, CollectionId::new())
    );
    denied!(
        Method::Get,
        "/store/regions",
        store::list_regions(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Get,
        "/store/regions/{id}",
        store::get_region(&mut tx, &ctx, RegionId::new())
    );
    denied!(
        Method::Get,
        "/store/currencies",
        store::list_currencies(&mut tx, &ctx)
    );
    denied!(
        Method::Get,
        "/store/currencies/{code}",
        store::get_currency(&mut tx, &ctx, "usd")
    );
    denied!(
        Method::Get,
        "/store/locales",
        store::list_locales(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/store/carts",
        store::create_cart(
            &mut tx,
            &ctx,
            "test-token",
            store::CreateCart {
                currency_code: "usd".to_string(),
                region_id: None,
                sales_channel_id: None,
                email: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/store/carts/{id}",
        store::get_cart(&mut tx, &ctx, CartId::new())
    );
    denied!(
        Method::Post,
        "/store/carts/{id}",
        store::update_cart(&mut tx, &ctx, CartId::new(), store::UpdateCart::default())
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/customer",
        store::set_cart_customer(&mut tx, &ctx, CartId::new())
    );
    denied!(
        Method::Get,
        "/store/carts/{id}/line-items",
        store::list_line_items(&mut tx, &ctx, CartId::new())
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/line-items",
        store::add_line_item(
            &mut tx,
            &ctx,
            CartId::new(),
            store::AddLineItem {
                variant_id: VariantId::new(),
                quantity: 1,
                selling_plan_id: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/bundle-items",
        store::add_bundle_item(
            &mut tx,
            &ctx,
            CartId::new(),
            store::AddBundleItem {
                variant_id: VariantId::new(),
                quantity: 1
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/line-items/{line_id}",
        store::update_line_item(
            &mut tx,
            &ctx,
            CartId::new(),
            LineItemId::new(),
            store::UpdateLineItem { quantity: 1 }
        )
    );
    denied!(
        Method::Delete,
        "/store/carts/{id}/line-items/{line_id}",
        store::remove_line_item(&mut tx, &ctx, CartId::new(), LineItemId::new())
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/promotions",
        store::apply_promotions(&mut tx, &ctx, CartId::new())
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/shipping-methods",
        store::set_shipping_method(
            &mut tx,
            &ctx,
            CartId::new(),
            store::ChooseShippingMethod {
                shipping_option_id: ShippingOptionId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/taxes",
        store::quote_taxes(
            &mut tx,
            &ctx,
            CartId::new(),
            store::DeliveryInput {
                country_code: "US".to_string(),
                province_code: None,
                city: None,
                postal_code: None,
                evidence: vec![],
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/tax-evidence",
        store::reprice_with_evidence(&mut tx, &ctx, CartId::new(), vec![])
    );
    denied!(
        Method::Get,
        "/store/shipping-options",
        store::list_shipping_options(
            &mut tx,
            &ctx,
            store::ListShippingOptions {
                cart_id: CartId::new(),
                country_code: "US".to_string(),
                province_code: None,
                city: None,
                postal_code: None,
                locale: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/store/shipping-options/{id}/calculate",
        store::calculate_shipping_option(
            &mut tx,
            &ctx,
            ShippingOptionId::new(),
            store::CalculateShipping {
                cart_id: CartId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/store/customers",
        store::create_customer(
            &mut tx,
            &ctx,
            store::CreateCustomer {
                email: "shopper@example.com".to_string(),
                first_name: None,
                last_name: None,
                phone: None,
                company_name: None,
            }
        )
    );
    denied!(Method::Get, "/store/customers/me", store::me(&mut tx, &ctx));
    denied!(
        Method::Post,
        "/store/customers/me",
        store::update_me(&mut tx, &ctx, store::UpdateMe::default())
    );
    denied!(
        Method::Get,
        "/store/customers/me/addresses",
        store::list_my_addresses(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Post,
        "/store/customers/me/addresses",
        store::add_my_address(&mut tx, &ctx, store::WriteAddress::default())
    );
    denied!(
        Method::Post,
        "/store/customers/me/addresses/{address_id}",
        store::update_my_address(
            &mut tx,
            &ctx,
            AddressId::new(),
            store::WriteAddress::default()
        )
    );
    denied!(
        Method::Delete,
        "/store/customers/me/addresses/{address_id}",
        store::delete_my_address(&mut tx, &ctx, AddressId::new())
    );
    denied!(
        Method::Post,
        "/store/customers/me/account-holders",
        store::save_my_account_holder(
            &mut tx,
            &ctx,
            store::SaveMyAccountHolder {
                provider_code: "stripe".to_string(),
                external_id: "ext_1".to_string(),
                email: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/store/customers/me/account-holders/{id}",
        store::delete_my_account_holder(&mut tx, &ctx, AccountHolderId::new())
    );
    denied!(
        Method::Get,
        "/store/orders",
        store::list_my_orders(&mut tx, &ctx, store::ListPage::default())
    );
    denied!(
        Method::Get,
        "/store/orders/{id}",
        store::get_my_order(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/store/orders/{id}/transfer/request",
        store::request_transfer(
            &mut tx,
            &ctx,
            OrderId::new(),
            store::RequestTransfer {
                to_email: "buyer@example.com".into(),
                expires_at: chrono::Utc::now(),
            }
        )
    );
    denied!(
        Method::Post,
        "/store/orders/{id}/transfer/decline",
        store::decline_transfer(
            &mut tx,
            &ctx,
            OrderId::new(),
            store::ClaimTransfer {
                token: "tok".to_string()
            }
        )
    );
    denied!(
        Method::Post,
        "/store/orders/{id}/transfer/cancel",
        store::cancel_transfer(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/store/returns",
        store::request_return(
            &mut tx,
            &ctx,
            store::RequestReturn {
                order_id: OrderId::new(),
                lines: vec![],
            }
        )
    );
    denied!(
        Method::Post,
        "/store/orders/{id}/transfer/accept",
        store::accept_transfer(
            &mut tx,
            &ctx,
            OrderId::new(),
            store::ClaimTransfer {
                token: "tok".to_string()
            }
        )
    );
    denied!(
        Method::Get,
        "/store/return-reasons",
        store::list_return_reasons(&mut tx, &ctx, store::ListReturnReasons::default())
    );
    denied!(
        Method::Get,
        "/store/return-reasons/{id}",
        store::get_return_reason(&mut tx, &ctx, uuid::Uuid::now_v7(), None)
    );
    denied!(
        Method::Post,
        "/store/payment-collections",
        store::create_payment_collection(
            &mut tx,
            &ctx,
            store::StartPayment {
                cart_id: CartId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/store/payment-collections/{id}/payment-sessions",
        store::create_payment_session(
            &mut tx,
            &ctx,
            PaymentCollectionId::new(),
            store::StartPaymentSession {
                cart_id: CartId::new(),
                provider_code: "stripe".to_string(),
                context: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/store/payment-providers",
        store::list_payment_providers(
            &mut tx,
            &ctx,
            store::ListPaymentProviders {
                cart_id: CartId::new()
            }
        )
    );

    // ------------------------------------------------------- credit.rs -----
    denied!(
        Method::Post,
        "/admin/gift-cards",
        credit::issue_gift_card(
            &mut tx,
            &ctx,
            credit::IssueGiftCard {
                balance: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
                customer_id: None,
                issued_order_id: None,
                expires_at: None,
                reason: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/gift-cards",
        credit::list_gift_cards(&mut tx, &ctx, credit::List::default())
    );
    denied!(
        Method::Post,
        "/admin/gift-cards/lookup",
        credit::find_gift_card(
            &mut tx,
            &ctx,
            credit::GiftCardCode {
                code: "CODE".to_string()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/gift-cards/{id}",
        credit::get_gift_card(&mut tx, &ctx, GiftCardId::new())
    );
    denied!(
        Method::Post,
        "/admin/gift-cards/{id}/disable",
        credit::disable_gift_card(&mut tx, &ctx, GiftCardId::new())
    );
    denied!(
        Method::Get,
        "/admin/gift-cards/{id}/transactions",
        credit::gift_card_movements(&mut tx, &ctx, GiftCardId::new(), credit::List::default())
    );
    denied!(
        Method::Post,
        "/admin/gift-cards/{id}/adjust",
        credit::adjust_gift_card(
            &mut tx,
            &ctx,
            GiftCardId::new(),
            credit::Adjustment {
                amount: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
                reason: "test".to_string(),
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}/store-credit",
        credit::get_store_credit(
            &mut tx,
            &ctx,
            CustomerId::new(),
            credit::BalanceQuery {
                currency_code: "USD".to_string()
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/customers/{id}/store-credit",
        credit::adjust_store_credit(
            &mut tx,
            &ctx,
            CustomerId::new(),
            credit::AdjustStoreCredit {
                amount: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
                reason: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/store-credits/{id}/transactions",
        credit::store_credit_movements(
            &mut tx,
            &ctx,
            StoreCreditId::new(),
            credit::List::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/store-credits/{id}/adjust",
        credit::adjust_store_credit_balance(
            &mut tx,
            &ctx,
            StoreCreditId::new(),
            credit::Adjustment {
                amount: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
                reason: "test".to_string(),
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/gift-cards",
        credit::apply_gift_card(
            &mut tx,
            &ctx,
            CartId::new(),
            credit::ApplyGiftCard {
                code: "CODE".to_string(),
                amount: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
            }
        )
    );
    denied!(
        Method::Post,
        "/store/carts/{id}/store-credit",
        credit::apply_store_credit(
            &mut tx,
            &ctx,
            CartId::new(),
            credit::ApplyStoreCredit {
                amount: credit::AmountIn {
                    amount: Decimal::ZERO,
                    currency_code: "USD".to_string(),
                },
            }
        )
    );
    denied!(
        Method::Get,
        "/store/carts/{id}/credits",
        credit::list_cart_credits(&mut tx, &ctx, CartId::new())
    );
    denied!(
        Method::Delete,
        "/store/carts/{id}/credits/{credit_id}",
        credit::remove_cart_credit(&mut tx, &ctx, CartId::new(), CartCreditId::new())
    );
    denied!(
        Method::Get,
        "/store/customers/me/store-credit",
        credit::my_store_credit(
            &mut tx,
            &ctx,
            credit::BalanceQuery {
                currency_code: "USD".to_string()
            }
        )
    );

    // --------------------------------------------------- subscription.rs ---
    denied!(
        Method::Post,
        "/admin/selling-plan-groups",
        subscription::create_plan_group(&mut tx, &ctx, subscription::CreatePlanGroup::default())
    );
    denied!(
        Method::Get,
        "/admin/selling-plan-groups",
        subscription::list_plan_groups(&mut tx, &ctx, subscription::List::default())
    );
    denied!(
        Method::Post,
        "/admin/selling-plan-groups/{id}/plans",
        subscription::create_plan(
            &mut tx,
            &ctx,
            SellingPlanGroupId::new(),
            subscription::CreatePlan::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/selling-plan-groups/{id}/plans",
        subscription::list_plans(
            &mut tx,
            &ctx,
            SellingPlanGroupId::new(),
            subscription::List::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/selling-plans/{id}",
        subscription::get_plan(&mut tx, &ctx, SellingPlanId::new())
    );
    denied!(
        Method::Post,
        "/admin/selling-plans/{id}/variants",
        subscription::attach_variant(
            &mut tx,
            &ctx,
            SellingPlanId::new(),
            subscription::AttachVariant {
                variant_id: VariantId::new()
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/subscriptions",
        subscription::create_subscription(
            &mut tx,
            &ctx,
            subscription::CreateSubscription {
                customer_id: CustomerId::new(),
                selling_plan_id: SellingPlanId::new(),
                currency_code: "USD".into(),
                region_id: None,
                sales_channel_id: None,
                account_holder_id: None,
                payment_method_reference: None,
                mandate_reference: None,
                mandate_accepted_at: None,
                shipping_address_id: None,
                billing_address_id: None,
                starts_at: None,
                lines: vec![],
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/subscriptions",
        subscription::list_subscriptions(&mut tx, &ctx, subscription::List::default())
    );
    denied!(
        Method::Get,
        "/admin/subscriptions/due",
        subscription::list_due(&mut tx, &ctx, subscription::ListDue::default())
    );
    denied!(
        Method::Get,
        "/admin/subscriptions/{id}",
        subscription::get_subscription(&mut tx, &ctx, SubscriptionId::new())
    );
    denied!(
        Method::Get,
        "/admin/subscriptions/{id}/events",
        subscription::list_events(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::List::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/cancel",
        subscription::cancel_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Cancel::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/pause",
        subscription::pause_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Pause::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/resume",
        subscription::resume_subscription(&mut tx, &ctx, SubscriptionId::new())
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/skip",
        subscription::skip_subscription(&mut tx, &ctx, SubscriptionId::new())
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/swap",
        subscription::swap_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Swap { lines: vec![] }
        )
    );
    denied!(
        Method::Post,
        "/admin/subscriptions/{id}/deliver",
        subscription::deliver_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Deliver {
                location_id: StockLocationId::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/subscriptions/due-deliveries",
        subscription::list_due_deliveries(&mut tx, &ctx, subscription::ListDue::default())
    );
    denied!(
        Method::Get,
        "/store/subscriptions",
        subscription::my_subscriptions(&mut tx, &ctx, subscription::List::default())
    );
    denied!(
        Method::Post,
        "/store/subscriptions/{id}/cancel",
        subscription::cancel_my_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Cancel::default()
        )
    );
    denied!(
        Method::Post,
        "/store/subscriptions/{id}/pause",
        subscription::pause_my_subscription(
            &mut tx,
            &ctx,
            SubscriptionId::new(),
            subscription::Pause::default()
        )
    );
    denied!(
        Method::Post,
        "/store/subscriptions/{id}/resume",
        subscription::resume_my_subscription(&mut tx, &ctx, SubscriptionId::new())
    );
    denied!(
        Method::Post,
        "/store/subscriptions/{id}/skip",
        subscription::skip_my_subscription(&mut tx, &ctx, SubscriptionId::new())
    );

    // ------------------------------------------------------ agreement.rs ---
    denied!(
        Method::Post,
        "/admin/agreements",
        agreement::publish_agreement(
            &mut tx,
            &ctx,
            agreement::PublishAgreement {
                kind: "other".into(),
                locale: "en".into(),
                body: "text".into(),
                effective_from: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/agreements",
        agreement::list_agreements(&mut tx, &ctx, agreement::ListAgreements::default())
    );
    denied!(
        Method::Get,
        "/admin/agreements/{id}",
        agreement::get_agreement(&mut tx, &ctx, AgreementVersionId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/invoices",
        agreement::list_invoices(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/agreements",
        agreement::order_agreements(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/agreements/{kind}",
        agreement::accepted_text(&mut tx, &ctx, OrderId::new(), "other")
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/withdrawal",
        agreement::withdrawal_windows(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/returns/{id}/withdrawal",
        agreement::notify_withdrawal(&mut tx, &ctx, ReturnId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/invoices",
        agreement::record_invoice(
            &mut tx,
            &ctx,
            OrderId::new(),
            agreement::RecordInvoice {
                number: "x".into(),
                external_id: None,
                provider: None,
                status: "requested".into(),
                total: dec!(0),
                currency_code: "TRY".into(),
                issued_at: None,
                document_url: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/invoices/{invoice_id}/credit-note",
        agreement::record_credit_note(
            &mut tx,
            &ctx,
            OrderId::new(),
            OrderInvoiceId::new(),
            agreement::RecordInvoice {
                number: "x".into(),
                external_id: None,
                provider: None,
                status: "requested".into(),
                total: dec!(0),
                currency_code: "TRY".into(),
                issued_at: None,
                document_url: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Patch,
        "/admin/invoices/{id}",
        agreement::set_invoice_status(
            &mut tx,
            &ctx,
            OrderInvoiceId::new(),
            agreement::SetInvoiceStatus {
                status: "requested".into()
            }
        )
    );
    denied!(
        Method::Post,
        "/store/orders/{id}/agreements",
        agreement::accept_agreement(
            &mut tx,
            &ctx,
            OrderId::new(),
            agreement::AcceptAgreement {
                agreement_version_id: AgreementVersionId::new(),
                accepted_at: None,
                ip: None,
                user_agent: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/store/orders/{id}/agreements/{kind}",
        agreement::my_accepted_text(&mut tx, &ctx, OrderId::new(), "other")
    );

    // --------------------------------------------------------- digital.rs --
    denied!(
        Method::Post,
        "/admin/variants/{id}/digital-content",
        digital::put_content(
            &mut tx,
            &ctx,
            VariantId::new(),
            digital::PutContent::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/variants/{id}/digital-content",
        digital::list_content(&mut tx, &ctx, VariantId::new())
    );
    denied!(
        Method::Delete,
        "/admin/digital-content/{id}",
        digital::delete_content(&mut tx, &ctx, DigitalContentId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/entitlements",
        digital::list_order_entitlements(&mut tx, &ctx, OrderId::new())
    );
    denied!(
        Method::Post,
        "/admin/orders/{id}/entitlements/revoke",
        digital::revoke_entitlements(
            &mut tx,
            &ctx,
            OrderId::new(),
            digital::RevokeEntitlements::default()
        )
    );
    denied!(
        Method::Get,
        "/store/entitlements",
        digital::my_entitlements(&mut tx, &ctx, digital::List::default())
    );
    denied!(
        Method::Post,
        "/store/entitlements/{id}/token",
        digital::create_token(&mut tx, &ctx, OrderEntitlementId::new())
    );
    denied!(
        Method::Post,
        "/store/downloads",
        digital::redeem(&mut tx, &ctx, digital::Redeem::default())
    );

    // -------------------------------------------------- inventory_lot.rs ---
    denied!(
        Method::Patch,
        "/admin/inventory-items/{id}/tracking",
        inventory_lot::set_tracking(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            inventory_lot::SetTracking {
                tracking_mode: tezgah::inventory::TrackingMode::Lot,
                allocation_strategy: tezgah::inventory::AllocationStrategy::Fifo,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-items/{id}/lots",
        inventory_lot::receive_lot(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            inventory_lot::ReceiveLot {
                location_id: StockLocationId::new(),
                lot_code: "LOT-1".into(),
                quantity: 1,
                expires_at: None,
                received_at: None,
                supplier_reference: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-items/{id}/lots",
        inventory_lot::list_lots(
            &mut tx,
            &ctx,
            InventoryItemId::new(),
            inventory_lot::ListLots::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-lots/expiring",
        inventory_lot::list_expiring_lots(
            &mut tx,
            &ctx,
            inventory_lot::ListExpiring {
                before: chrono::Utc::now(),
                after: None,
                limit: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-lots/{id}",
        inventory_lot::get_lot(&mut tx, &ctx, InventoryLotId::new())
    );
    denied!(
        Method::Patch,
        "/admin/inventory-lots/{id}",
        inventory_lot::adjust_lot(
            &mut tx,
            &ctx,
            InventoryLotId::new(),
            inventory_lot::AdjustLot {
                delta: -1,
                reason: None
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/inventory-lots/{id}/orders",
        inventory_lot::orders_for_lot(
            &mut tx,
            &ctx,
            InventoryLotId::new(),
            inventory_lot::ListRecall::default()
        )
    );
    denied!(
        Method::Post,
        "/admin/inventory-lots/{id}/reservations",
        inventory_lot::reserve_from_lot(
            &mut tx,
            &ctx,
            InventoryLotId::new(),
            inventory_lot::ReserveFromLot {
                quantity: 1,
                line_item_id: None,
                expires_at: None,
            }
        )
    );

    // -------------------------------------------------- order_basket.rs ----
    denied!(
        Method::Post,
        "/admin/order-baskets",
        order_basket::open_basket(
            &mut tx,
            &ctx,
            order_basket::OpenBasket {
                customer_id: None,
                currency_code: "TRY".into(),
                email: None,
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/order-baskets/{id}",
        order_basket::get_basket(&mut tx, &ctx, OrderBasketId::new())
    );
    denied!(
        Method::Post,
        "/admin/order-baskets/{id}/payment-collection",
        order_basket::attach_payment_collection(
            &mut tx,
            &ctx,
            OrderBasketId::new(),
            order_basket::AttachPaymentCollection {
                payment_collection_id: PaymentCollectionId::new()
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/order-baskets/{id}/orders",
        order_basket::basket_orders(
            &mut tx,
            &ctx,
            OrderBasketId::new(),
            order_basket::ListBasketOrders::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/order-baskets/{id}/carts",
        order_basket::basket_carts(
            &mut tx,
            &ctx,
            OrderBasketId::new(),
            order_basket::ListBasketOrders::default()
        )
    );
    denied!(
        Method::Get,
        "/admin/carts",
        order_basket::list_carts(&mut tx, &ctx, order_basket::ListCarts::default())
    );

    // -------------------------------------------------------- payout.rs ----
    denied!(
        Method::Post,
        "/admin/commission-rules",
        payout::set_commission_rule(
            &mut tx,
            &ctx,
            payout::SetCommissionRule {
                category_id: None,
                kind: "percentage".into(),
                value: Decimal::ZERO,
                currency_code: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/commission-rules",
        payout::commission_rules(&mut tx, &ctx, payout::ListQuery::default())
    );
    denied!(
        Method::Delete,
        "/admin/commission-rules/{id}",
        payout::remove_commission_rule(&mut tx, &ctx, CommissionRuleId::new())
    );
    denied!(
        Method::Get,
        "/admin/orders/{id}/payout-lines",
        payout::order_payout_lines(&mut tx, &ctx, OrderId::new(), payout::ListQuery::default())
    );
    denied!(
        Method::Get,
        "/admin/payouts",
        payout::payouts(&mut tx, &ctx, payout::ListQuery::default())
    );
    denied!(
        Method::Post,
        "/admin/payouts",
        payout::create_payout(
            &mut tx,
            &ctx,
            payout::CreatePayout {
                currency_code: "TRY".into(),
                reference: "ref-1".into(),
                reference_id: uuid::Uuid::now_v7(),
                metadata: None,
            }
        )
    );
    denied!(
        Method::Get,
        "/admin/payout-balance/{currency_code}",
        payout::balance(&mut tx, &ctx, "TRY".to_string())
    );

    // ---------------------------------------------------- tax_identity.rs --
    denied!(
        Method::Get,
        "/admin/tax-registrations",
        tax_identity::list_registrations(&mut tx, &ctx)
    );
    denied!(
        Method::Post,
        "/admin/tax-registrations",
        tax_identity::register_shop(
            &mut tx,
            &ctx,
            tax_identity::RegisterShop {
                country_code: "TR".into(),
                scheme: "domestic".into(),
                tax_id: None,
                is_home: true,
                valid_from: None,
                valid_until: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/tax-registrations/{id}",
        tax_identity::delete_registration(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}/tax-ids",
        tax_identity::list_tax_ids(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Post,
        "/admin/customers/{id}/tax-ids",
        tax_identity::record_tax_id(
            &mut tx,
            &ctx,
            CustomerId::new(),
            tax_identity::RecordTaxId {
                tax_id: "12345".into(),
                tax_id_type: "vat".into(),
                tax_id_country: "TR".into(),
                validated_at: None,
                evidence: None,
            }
        )
    );
    denied!(
        Method::Delete,
        "/admin/tax-ids/{id}",
        tax_identity::delete_tax_id(&mut tx, &ctx, uuid::Uuid::now_v7())
    );
    denied!(
        Method::Get,
        "/admin/customers/{id}/tax-exemptions",
        tax_identity::list_exemptions(&mut tx, &ctx, CustomerId::new())
    );
    denied!(
        Method::Post,
        "/admin/customers/{id}/tax-exemptions",
        tax_identity::grant_exemption(
            &mut tx,
            &ctx,
            CustomerId::new(),
            tax_identity::GrantExemption {
                kind: "certificate".into(),
                reason_code: None,
                certificate_reference: None,
                country_code: "TR".into(),
                province_code: None,
                valid_from: None,
                valid_until: None,
                verified_at: None,
                evidence: None,
            }
        )
    );
    denied!(
        Method::Post,
        "/admin/tax-exemptions/{id}/revoke",
        tax_identity::revoke_exemption(
            &mut tx,
            &ctx,
            uuid::Uuid::now_v7(),
            tax_identity::RevokeExemption::default()
        )
    );

    // ------------------------------------------------------ completeness ---
    for route in routes() {
        if covered.contains(&(route.method, route.path)) {
            continue;
        }
        assert!(
            tolerated(route.method, route.path),
            "{} {} is declared in routes() but the deny-matrix never calls it \
             — either call it above or add a reasoned, shrinking TOLERATED entry",
            route.method.as_str(),
            route.path
        );
    }
    for (method, path, _) in TOLERATED {
        assert!(
            routes()
                .iter()
                .any(|r| r.method == *method && r.path == *path),
            "TOLERATED names {} {}, which is not a route any more; remove it",
            method.as_str(),
            path
        );
        assert!(
            !covered.contains(&(*method, *path)),
            "{} {} is both called above and TOLERATED; drop it from TOLERATED",
            method.as_str(),
            path
        );
    }

    assert!(
        allowed.is_empty(),
        "these were reached by somebody the host refuses everything to:\n  {}",
        allowed.join("\n  ")
    );
    assert!(!refused.is_empty(), "nothing was actually called");

    drop(tx);
    shop.close().await;
}
