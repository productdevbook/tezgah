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
//! each one comes back denied. That is the half that proves the declaration is
//! not decoration — a handler that never asked would return rows instead.

mod common;

use common::{Doorman, Shop};
use tezgah::api::admin_rest;
use tezgah::api::{Method, Route, Surface, routes};
use tezgah::id::{
    AddressId, CampaignId, CustomerGroupId, CustomerId, PromotionId, PublishableKeyId, RegionId,
    TaxRateId, TaxRegionId, WorkflowRunId,
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
        "payment",
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

/// The half a table cannot prove: that the handler asks at all.
///
/// One call per action per domain rather than one per route, because the
/// permission is asked for by the domain call the handlers share — a route that
/// did not ask would have to be one that reached the database another way, and
/// there is no other way in this crate.
#[tokio::test]
async fn a_handler_reached_by_somebody_with_no_permission_is_refused() {
    let shop = Shop::open().await;
    let doorman = Doorman;
    let ctx = shop.ctx_as(
        Actor::Staff {
            id: uuid::Uuid::now_v7(),
        },
        &doorman,
    );
    let mut tx = shop.begin().await;

    let customer = CustomerId::new();
    let group = CustomerGroupId::new();
    let address = AddressId::new();
    let promotion = PromotionId::new();
    let region = TaxRegionId::new();
    let rate = TaxRateId::new();

    let mut refused = Vec::new();
    let mut allowed = Vec::new();

    macro_rules! denied {
        ($what:literal, $call:expr) => {
            match $call.await {
                Err(error) if error.is_denied() => refused.push($what),
                Err(error) => allowed.push(format!("{}: {:?}", $what, error.code())),
                Ok(_) => allowed.push(format!("{}: answered without asking", $what)),
            }
        };
    }

    denied!(
        "GET /admin/customers",
        admin_rest::list_customers(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "POST /admin/customers",
        admin_rest::create_customer(&mut tx, &ctx, admin_rest::CreateCustomer::default())
    );
    denied!(
        "GET /admin/customers/{id}",
        admin_rest::get_customer(&mut tx, &ctx, customer)
    );
    denied!(
        "DELETE /admin/customers/{id}",
        admin_rest::delete_customer(&mut tx, &ctx, customer)
    );
    denied!(
        "GET /admin/customers/{id}/export",
        admin_rest::export_customer(&mut tx, &ctx, customer)
    );
    denied!(
        "POST /admin/customers/{id}/erase",
        admin_rest::erase_customer(&mut tx, &ctx, customer)
    );
    denied!(
        "GET /admin/customers/{id}/addresses",
        admin_rest::list_addresses(&mut tx, &ctx, customer, admin_rest::List::default())
    );
    denied!(
        "POST /admin/customers/{id}/addresses",
        admin_rest::add_address(&mut tx, &ctx, customer, admin_rest::WriteAddress::default())
    );
    denied!(
        "DELETE /admin/customers/{id}/addresses/{address_id}",
        admin_rest::delete_address(&mut tx, &ctx, address)
    );
    denied!(
        "GET /admin/customer-groups",
        admin_rest::list_groups(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "DELETE /admin/customer-groups/{id}",
        admin_rest::delete_group(&mut tx, &ctx, group)
    );
    denied!(
        "GET /admin/customer-groups/{id}/customers",
        admin_rest::list_group_members(&mut tx, &ctx, group, admin_rest::List::default())
    );
    denied!(
        "GET /admin/promotions",
        admin_rest::list_promotions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "GET /admin/promotions/{id}",
        admin_rest::get_promotion(&mut tx, &ctx, promotion)
    );
    denied!(
        "DELETE /admin/promotions/{id}",
        admin_rest::delete_promotion(&mut tx, &ctx, promotion)
    );
    denied!(
        "GET /admin/campaigns",
        admin_rest::list_campaigns(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "GET /admin/tax-regions",
        admin_rest::list_tax_regions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "GET /admin/tax-regions/{id}",
        admin_rest::get_tax_region(&mut tx, &ctx, region)
    );
    denied!(
        "DELETE /admin/tax-regions/{id}",
        admin_rest::delete_tax_region(&mut tx, &ctx, region)
    );
    denied!(
        "GET /admin/tax-rates",
        admin_rest::list_tax_rates(&mut tx, &ctx, admin_rest::ListTaxRates::default())
    );
    denied!(
        "DELETE /admin/tax-rates/{id}",
        admin_rest::delete_tax_rate(&mut tx, &ctx, rate)
    );
    denied!(
        "GET /admin/tax-rates/{id}/rules",
        admin_rest::list_tax_rate_rules(&mut tx, &ctx, rate)
    );
    denied!(
        "GET /admin/regions",
        admin_rest::list_regions(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "PATCH /admin/regions/{id}",
        admin_rest::update_region(
            &mut tx,
            &ctx,
            RegionId::new(),
            admin_rest::UpdateRegion::default()
        )
    );
    denied!(
        "PATCH /admin/tax-regions/{id}",
        admin_rest::update_tax_region(
            &mut tx,
            &ctx,
            region,
            admin_rest::UpdateTaxRegion::default()
        )
    );
    denied!(
        "GET /admin/tax-rates/{id}",
        admin_rest::get_tax_rate(&mut tx, &ctx, rate)
    );
    denied!(
        "PATCH /admin/tax-rates/{id}",
        admin_rest::update_tax_rate(&mut tx, &ctx, rate, admin_rest::UpdateTaxRate::default())
    );
    denied!(
        "GET /admin/campaigns/{id}",
        admin_rest::get_campaign(&mut tx, &ctx, CampaignId::new())
    );
    denied!(
        "PATCH /admin/campaigns/{id}",
        admin_rest::update_campaign(
            &mut tx,
            &ctx,
            CampaignId::new(),
            admin_rest::UpdateCampaign::default()
        )
    );
    denied!(
        "POST /admin/campaigns/{id}/promotions",
        admin_rest::add_campaign_promotion(
            &mut tx,
            &ctx,
            CampaignId::new(),
            admin_rest::AttachPromotion {
                promotion_id: promotion
            }
        )
    );
    denied!(
        "DELETE /admin/campaigns/{id}/promotions/{promotion_id}",
        admin_rest::remove_campaign_promotion(&mut tx, &ctx, CampaignId::new(), promotion)
    );
    denied!(
        "PATCH /admin/promotions/{id}",
        admin_rest::update_promotion(
            &mut tx,
            &ctx,
            promotion,
            admin_rest::UpdatePromotion::default()
        )
    );
    denied!(
        "GET /admin/sales-channels",
        admin_rest::list_sales_channels(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "GET /admin/publishable-api-keys",
        admin_rest::list_publishable_keys(&mut tx, &ctx, admin_rest::List::default())
    );
    denied!(
        "GET /admin/publishable-api-keys/{id}/sales-channels",
        admin_rest::list_key_sales_channels(&mut tx, &ctx, PublishableKeyId::new())
    );
    denied!(
        "GET /admin/currencies",
        admin_rest::list_currencies(&mut tx, &ctx)
    );
    // The one handler that asks for itself: the runner takes no `Permit`, so
    // the route is where the question is put.
    denied!(
        "GET /admin/workflows-executions/{id}",
        admin_rest::get_workflow_run(&shop.pool, &ctx, WorkflowRunId::new())
    );

    assert!(
        allowed.is_empty(),
        "these were reached by somebody the host refuses everything to:\n  {}",
        allowed.join("\n  ")
    );
    assert!(!refused.is_empty(), "nothing was actually called");

    drop(tx);
    shop.close().await;
}
