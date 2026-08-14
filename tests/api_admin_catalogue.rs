//! The back office's catalogue surface, against a real Postgres.
//!
//! What is worth asserting here is not that the handlers call the domain — it
//! is the four things the surface itself decides: a draft is visible where the
//! storefront hides it, an actor the host refuses gets nothing, a listing is
//! paged whatever it is asked for, and a body with a field nobody declared is
//! refused rather than quietly ignored.

mod common;

use common::{Doorman, Shop};
use tezgah::api::admin_catalogue as admin;
use tezgah::api::store as storefront;
use tezgah::catalogue::ProductStatus;
use tezgah::ports::Actor;

fn draft(handle: &str, title: &str) -> admin::CreateProduct {
    admin::CreateProduct {
        handle: handle.into(),
        title: title.into(),
        ..admin::CreateProduct::default()
    }
}

#[tokio::test]
async fn a_draft_is_visible_here_and_nowhere_else() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = admin::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");
    assert_eq!(made.status, ProductStatus::Draft);

    let listed = admin::list_products(&mut tx, &ctx, admin::ListProducts::default())
        .await
        .expect("to list");
    assert_eq!(listed.len(), 1, "an admin listing shows a draft");

    let read = admin::get_product(&mut tx, &ctx, made.id)
        .await
        .expect("to read a draft back");
    assert_eq!(read.handle, "kilim");

    let refused = storefront::get_product(&mut tx, &ctx, "kilim").await;
    assert!(
        refused.is_err_and(|err| err.is_not_found()),
        "the storefront does not admit a draft exists"
    );

    let shopper = storefront::list_products(&mut tx, &ctx, storefront::ListProducts::default())
        .await
        .expect("to list");
    assert!(shopper.is_empty());

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn asking_for_one_status_narrows_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = admin::create_product(&mut tx, &ctx, draft("kilim", "A kilim"))
        .await
        .expect("a product");
    admin::create_product(&mut tx, &ctx, draft("cezve", "A cezve"))
        .await
        .expect("another");
    admin::publish_product(&mut tx, &ctx, made.id)
        .await
        .expect("to publish");

    let published = admin::list_products(
        &mut tx,
        &ctx,
        admin::ListProducts {
            status: Some(ProductStatus::Published),
            ..admin::ListProducts::default()
        },
    )
    .await
    .expect("to list");
    assert_eq!(published.len(), 1);

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn an_actor_the_host_refuses_gets_nothing() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    let doorman = Doorman;
    let denied = shop.ctx_as(
        Actor::Staff {
            id: uuid::Uuid::now_v7(),
        },
        &doorman,
    );

    let listed = admin::list_products(&mut tx, &denied, admin::ListProducts::default()).await;
    assert!(listed.is_err_and(|err| err.is_denied()));

    let made = admin::create_product(&mut tx, &denied, draft("kilim", "A kilim")).await;
    assert!(made.is_err_and(|err| err.is_denied()));

    let locations =
        admin::list_stock_locations(&mut tx, &denied, admin::ListQuery::default()).await;
    assert!(locations.is_err_and(|err| err.is_denied()));

    let lists = admin::list_price_lists(&mut tx, &denied, admin::ListQuery::default()).await;
    assert!(lists.is_err_and(|err| err.is_denied()));

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_listing_is_paged_and_a_greedy_limit_is_brought_down() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    for at in 0..5 {
        admin::create_product(&mut tx, &ctx, draft(&format!("kilim-{at}"), "A kilim"))
            .await
            .expect("a product");
    }

    let first = admin::list_products(
        &mut tx,
        &ctx,
        admin::ListProducts {
            limit: Some(2),
            ..admin::ListProducts::default()
        },
    )
    .await
    .expect("to list");
    assert_eq!(first.len(), 2);
    let next = first.next.clone().expect("another page");

    let second = admin::list_products(
        &mut tx,
        &ctx,
        admin::ListProducts {
            after: Some(next),
            limit: Some(2),
            ..admin::ListProducts::default()
        },
    )
    .await
    .expect("to list on");
    assert_eq!(second.len(), 2);

    let seen: Vec<_> = first.items.iter().map(|p| p.id).collect();
    assert!(
        second.items.iter().all(|p| !seen.contains(&p.id)),
        "a second page repeats nothing from the first"
    );

    // Clamped rather than refused: the ceiling is the crate's, not the caller's.
    let greedy = admin::list_products(
        &mut tx,
        &ctx,
        admin::ListProducts {
            limit: Some(100_000),
            ..admin::ListProducts::default()
        },
    )
    .await
    .expect("to list");
    assert_eq!(greedy.len(), 5);
    assert!(greedy.next.is_none());

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn a_cursor_that_is_not_one_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let asked = admin::list_products(
        &mut tx,
        &ctx,
        admin::ListProducts {
            after: Some("not-a-cursor".into()),
            ..admin::ListProducts::default()
        },
    )
    .await;
    assert!(asked.is_err());

    drop(tx);
    shop.close().await;
}

#[tokio::test]
async fn another_scope_sees_none_of_it() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    let made = admin::create_product(&mut tx, &shop.ctx(), draft("kilim", "A kilim"))
        .await
        .expect("a product");
    tx.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let listed = admin::list_products(&mut theirs, &shop.theirs(), admin::ListProducts::default())
        .await
        .expect("to list");
    assert!(
        listed.is_empty(),
        "a draft belongs to the shop that wrote it"
    );

    let read = admin::get_product(&mut theirs, &shop.theirs(), made.id).await;
    assert!(read.is_err_and(|err| err.is_not_found()));

    drop(theirs);
    shop.close().await;
}

#[tokio::test]
async fn stock_is_set_read_back_and_adjusted() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let location = admin::create_stock_location(
        &mut tx,
        &ctx,
        admin::CreateStockLocation {
            name: "The shed".into(),
        },
    )
    .await
    .expect("a location");

    let item = admin::create_inventory_item(
        &mut tx,
        &ctx,
        admin::CreateInventoryItem {
            sku: Some("KIL-1".into()),
            title: Some("A kilim".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    let level = admin::set_stock(
        &mut tx,
        &ctx,
        item.id,
        admin::SetStock {
            location_id: location.id,
            stocked_quantity: 10,
            incoming_quantity: 0,
        },
    )
    .await
    .expect("to count the stock");
    assert_eq!(level.stocked_quantity, 10);

    let moved = admin::adjust_stock(
        &mut tx,
        &ctx,
        item.id,
        location.id,
        admin::AdjustStock {
            delta: -3,
            reason: Some("broken".into()),
        },
    )
    .await
    .expect("to adjust");
    assert_eq!(moved.stocked_quantity, 7);

    let levels = admin::list_levels(&mut tx, &ctx, item.id, admin::ListQuery::default())
        .await
        .expect("to list levels");
    assert_eq!(levels.len(), 1);

    drop(tx);
    shop.close().await;
}

// ---------------------------------------------------------------------------
// Inputs, which need no database
// ---------------------------------------------------------------------------

#[test]
fn a_field_nobody_declared_is_refused_rather_than_ignored() {
    let body = serde_json::json!({
        "handle": "kilim",
        "title": "A kilim",
        "stauts": "published"
    });
    let read = serde_json::from_value::<admin::CreateProduct>(body);
    assert!(
        read.is_err(),
        "a misspelled field silently dropped is a change that never happened"
    );

    let query = serde_json::json!({ "limit": 10, "offset": 20 });
    assert!(serde_json::from_value::<admin::ListQuery>(query).is_err());

    let patch = serde_json::json!({ "titel": "A kilim" });
    assert!(serde_json::from_value::<admin::UpdateProduct>(patch).is_err());

    let stock = serde_json::json!({
        "location_id": uuid::Uuid::now_v7(),
        "stocked_quantity": 1,
        "incoming_quantity": 0,
        "note": "hello"
    });
    assert!(serde_json::from_value::<admin::SetStock>(stock).is_err());
}

/// Absent, `null` and a value are three answers, not two: one leaves the link
/// alone, one clears it, one sets it.
#[test]
fn a_nullable_link_tells_absent_apart_from_cleared() {
    let untouched: admin::UpdateProduct =
        serde_json::from_value(serde_json::json!({})).expect("an empty patch");
    assert!(untouched.product_collection_id.is_none());

    let cleared: admin::UpdateProduct =
        serde_json::from_value(serde_json::json!({ "product_collection_id": null }))
            .expect("a clearing patch");
    assert_eq!(cleared.product_collection_id, Some(None));
}

#[test]
fn an_amount_without_a_currency_is_refused() {
    let body: admin::UpdatePrice =
        serde_json::from_value(serde_json::json!({ "amount": "10.00" })).expect("a patch");
    assert!(body.amount.is_some());
    assert!(body.currency_code.is_none());
}

#[test]
fn every_route_this_module_declares_is_on_the_admin_surface() {
    use tezgah::api::{Method, Surface, routes};

    let mine: Vec<_> = routes()
        .into_iter()
        .filter(|route| route.path.starts_with("/admin/product"))
        .collect();
    assert!(!mine.is_empty());

    for route in mine {
        assert_eq!(route.surface, Surface::Admin, "{}", route.path);
        if route.method == Method::Get {
            assert_eq!(route.action, tezgah::ports::Action::View, "{}", route.path);
        }
    }
}
