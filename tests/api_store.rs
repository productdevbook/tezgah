//! The storefront surface against a real Postgres.
//!
//! The two rules the surface exists to keep are the two worth testing: nothing
//! unpublished is visible, and nobody reaches somebody else's cart. The rest is
//! the paging ceiling, the shape of what comes in, and the scope boundary — all
//! of which have been wrong in a way that only shows up when it is run.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::Shop;
use tezgah::api::store::{self, CreateCart, ListPage, ListProducts};
use tezgah::catalogue::{self, NewProduct};
use tezgah::id::CustomerId;
use tezgah::page::MAX_LIMIT;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, Host, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use uuid::Uuid;

/// A host that answers ownership the way a real one would, and says yes to
/// everything else.
///
/// It grants a cart whose owner is unknown on purpose: the domain call asks
/// that way, so a refusal here would prove nothing about the surface. What the
/// test is watching for is the second question, the one the surface asks with
/// the owner it has just read.
#[derive(Debug, Default)]
struct OnlyOwner;

impl Authorizer for OnlyOwner {
    fn authorize(&self, actor: &Actor, _: Action, resource: &Resource) -> tezgah::Result<Permit> {
        let owner = match resource {
            Resource::Cart { customer, .. } | Resource::Order { customer, .. } => *customer,
            _ => None,
        };

        match (owner, actor) {
            (None, _) => Ok(Permit::granted()),
            (Some(owner), Actor::Customer { id }) if owner == *id => Ok(Permit::granted()),
            (Some(_), _) => Err(tezgah::Error::denied()),
        }
    }
}

impl Clock for OnlyOwner {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for OnlyOwner {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for OnlyOwner {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for OnlyOwner {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

async fn a_customer(tx: &mut Tx<'_>, shop: &Shop, email: &str) -> tezgah::Result<CustomerId> {
    let made = tezgah::customer::create(
        tx,
        &shop.ctx(),
        tezgah::customer::NewCustomer {
            email: Some(email.into()),
            first_name: None,
            last_name: None,
            phone: None,
            company_name: None,
            has_account: true,
            metadata: None,
        },
    )
    .await?;

    Ok(made.id)
}

fn draft(handle: &str) -> NewProduct {
    NewProduct {
        handle: handle.into(),
        title: format!("A {handle}"),
        ..NewProduct::default()
    }
}

#[tokio::test]
async fn a_draft_is_not_here_and_a_published_one_is() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kilim")).await?;

    let hidden = store::get_product(&mut tx, &ctx, "kilim")
        .await
        .expect_err("a draft is not on the storefront");
    assert!(
        hidden.is_not_found(),
        "a draft answers not_found rather than denied, so a stranger learns nothing"
    );
    assert!(!hidden.is_denied());

    let listed = store::list_products(&mut tx, &ctx, ListProducts::default()).await?;
    assert!(listed.is_empty(), "a draft is not listed either");

    catalogue::publish_product(&mut tx, &ctx, made.id).await?;

    let shown = store::get_product(&mut tx, &ctx, "kilim").await?;
    assert_eq!(shown.handle, "kilim");

    let listed = store::list_products(&mut tx, &ctx, ListProducts::default()).await?;
    assert_eq!(listed.len(), 1);

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_variant_of_a_draft_is_not_here_either() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let made = catalogue::create_product(&mut tx, &ctx, draft("kettle")).await?;
    let variant = catalogue::create_variant(
        &mut tx,
        &ctx,
        made.id,
        catalogue::NewVariant {
            title: "One size".into(),
            ..catalogue::NewVariant::default()
        },
    )
    .await?;

    assert!(
        store::get_variant(&mut tx, &ctx, variant.id)
            .await
            .expect_err("its product is a draft")
            .is_not_found()
    );

    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    let seen = store::get_variant(&mut tx, &ctx, variant.id).await?;
    assert_eq!(seen.id, variant.id);

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_is_not_reached_by_somebody_else() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let host: Arc<OnlyOwner> = Arc::new(OnlyOwner);
    let mut tx = shop.begin().await;

    let mine = a_customer(&mut tx, &shop, "shopper@example.test").await?;
    let theirs = a_customer(&mut tx, &shop, "somebody@example.test").await?;

    let ctx = shop.ctx_as(
        Actor::Customer { id: mine.as_uuid() },
        host.as_ref() as &dyn Host,
    );
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: theirs.as_uuid(),
        },
        host.as_ref() as &dyn Host,
    );

    let held = store::create_cart(
        &mut tx,
        &ctx,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;
    assert_eq!(held.customer_id, Some(mine));

    let read = store::get_cart(&mut tx, &ctx, held.id).await?;
    assert_eq!(read.id, held.id);

    let refused = store::get_cart(&mut tx, &stranger, held.id)
        .await
        .expect_err("somebody else's cart is not theirs to read");
    assert!(
        refused.is_denied(),
        "the surface handed the host the cart's owner and the host said no"
    );

    assert!(
        store::update_cart(&mut tx, &stranger, held.id, store::UpdateCart::default())
            .await
            .expect_err("nor to write")
            .is_denied()
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_signed_out_shopper_has_no_account_to_read() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    assert!(
        store::me(&mut tx, &ctx)
            .await
            .expect_err("nobody is signed in")
            .is_denied()
    );
    assert!(
        store::list_my_orders(&mut tx, &ctx, ListPage::default())
            .await
            .expect_err("nor are there orders to list")
            .is_denied()
    );

    drop(tx);
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_listing_pages_and_stops_at_the_ceiling() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let over_the_ceiling = MAX_LIMIT + 1;
    for at in 0..over_the_ceiling {
        let made = catalogue::create_product(&mut tx, &ctx, draft(&format!("thing-{at}"))).await?;
        catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    }

    let asked_for_everything = store::list_products(
        &mut tx,
        &ctx,
        ListProducts {
            limit: Some(100_000),
            ..ListProducts::default()
        },
    )
    .await?;
    assert_eq!(
        asked_for_everything.len(),
        MAX_LIMIT as usize,
        "a limit beyond the ceiling is brought down to it rather than refused"
    );
    let next = asked_for_everything
        .next
        .clone()
        .expect("a full page offers the next one");

    let rest = store::list_products(
        &mut tx,
        &ctx,
        ListProducts {
            after: Some(next),
            limit: Some(100_000),
            ..ListProducts::default()
        },
    )
    .await?;
    assert_eq!(rest.len(), 1, "the page after the ceiling holds the rest");
    assert!(rest.next.is_none());

    let nonsense = store::list_products(
        &mut tx,
        &ctx,
        ListProducts {
            after: Some("not-a-cursor".into()),
            ..ListProducts::default()
        },
    )
    .await
    .expect_err("that is not a cursor");
    assert_eq!(nonsense.code(), "invalid");

    drop(tx);
    shop.close().await;
    Ok(())
}

#[test]
fn an_input_refuses_a_field_it_does_not_know() {
    let good: Result<ListProducts, _> = serde_json::from_str(r#"{"limit": 5, "after": "abc"}"#);
    assert!(good.is_ok(), "what the surface documents still parses");

    let sneaky: Result<ListProducts, _> =
        serde_json::from_str(r#"{"limit": 5, "status": "draft"}"#);
    assert!(
        sneaky.is_err(),
        "an unknown field is a mistake or an attempt, and either way not a default"
    );

    let cart: Result<CreateCart, _> =
        serde_json::from_str(r#"{"currency_code": "TRY", "customer_id": "someone else"}"#);
    assert!(
        cart.is_err(),
        "a storefront does not get to say whose cart it is"
    );

    let line: Result<store::AddLineItem, _> = serde_json::from_str(
        r#"{"variant_id": "00000000-0000-0000-0000-000000000000", "quantity": 1,
            "unit_price": "0.01"}"#,
    );
    assert!(line.is_err(), "nor what a thing costs");
}

#[tokio::test]
async fn another_scope_sees_none_of_it() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;

    let ctx = shop.ctx();
    let made = catalogue::create_product(&mut tx, &ctx, draft("carpet")).await?;
    catalogue::publish_product(&mut tx, &ctx, made.id).await?;
    tx.commit().await?;

    let mut elsewhere = shop.begin_as(shop.elsewhere).await;
    let theirs = shop.theirs();

    assert!(
        store::get_product(&mut elsewhere, &theirs, "carpet")
            .await
            .expect_err("that is another shop's product")
            .is_not_found()
    );
    assert!(
        store::list_products(&mut elsewhere, &theirs, ListProducts::default())
            .await?
            .is_empty()
    );
    assert!(
        store::get_variant(&mut elsewhere, &theirs, uuid_variant())
            .await
            .expect_err("nor is anything else")
            .is_not_found()
    );

    drop(elsewhere);
    shop.close().await;
    Ok(())
}

fn uuid_variant() -> tezgah::id::VariantId {
    tezgah::id::VariantId::from_uuid(Uuid::nil())
}

#[test]
fn every_storefront_route_is_under_the_storefront_prefix() {
    let store_routes: Vec<_> = tezgah::api::routes()
        .into_iter()
        .filter(|route| route.surface == tezgah::api::Surface::Store)
        .collect();

    assert!(!store_routes.is_empty());
    for route in &store_routes {
        assert!(
            route.path.starts_with("/store/"),
            "{} is on the storefront surface but not under /store",
            route.path
        );
        assert!(!route.summary.is_empty());
    }
}
