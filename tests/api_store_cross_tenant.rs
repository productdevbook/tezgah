//! "Shopper B, reaching for shopper A's own thing."
//!
//! The permission matrix in `api_permissions.rs` proves a different claim: that
//! every route asks a host at all. It answers that with one host that refuses
//! everything, so it cannot tell an owner from a stranger — both look the same
//! to a host that says no to both. The mistake this file exists to catch does
//! not show up there: three routes this session, each found by hand, each with
//! a host that answers correctly and a handler that never put the right
//! question to it. #82 opened a payment session on somebody else's collection
//! because the cart in the body, not the collection in the path, was what got
//! asked about. #132 read a variant by id past a channel filter that only
//! ever guarded the listing. #135 let a cart take a line for a variant its own
//! channel had never been given.
//!
//! So: two shoppers, A and B, each with a cart, an order, an address, a
//! payment collection, a subscription and a downloadable entitlement of their
//! own. Every [`Surface::Store`] route that can name somebody's resource by id
//! is called twice — once as A on A's own id, once as B on A's id — and the
//! second call has to refuse. The first call is not a courtesy: a route that
//! only ever tests the refusal passes by never trying the thing it exists to
//! allow, which proves nothing about whether it still works.
//!
//! # What this proves and what it does not
//!
//! It proves that a route naming a cart, an order, an address, a payment
//! collection, a subscription or an entitlement by id refuses a customer who
//! is not its owner, and grants the one who is. It does not prove anything
//! about a route with no id to probe — `GET /store/orders` lists the caller's
//! own, `GET /store/customers/me` has no other account to ask for — those are
//! safe by construction and are named in [`SELF_ONLY`] rather than exercised
//! here. It does not prove anything about the catalogue and shop-configuration
//! reads in [`PUBLIC`]: nothing on the storefront is scoped to a customer, and
//! the channel axis (#132, #135) already has its own tests in `api_store.rs`.
//! And it does not replace [`TOLERATED`]'s two heavy-fixture routes or the
//! transfer and download routes, which are gated by a possession token rather
//! than a customer id — the id in their path names the resource, not who may
//! reach it, and the token is the actual key. Those are exercised for "wrong
//! or no token refuses" rather than "wrong customer refuses", because that is
//! the question their own design puts to a caller.
//!
//! `denied` or `not_found`? A route that has already loaded its own row before
//! asking — every one of these, since the owner has to be read before it can
//! be compared — hands the host a real owner, and this file's [`Owner`] host
//! answers `denied` for a mismatch. That is true even of
//! `create_payment_session`: #82's own fix compares the collection's
//! `cart_id` against the one in the body, but `payment::collection` already
//! asks about the collection's real owner first, so a host that polices
//! `Resource::Payment` — this one does — refuses before that comparison is
//! ever reached. `not_found` is reserved for what a stranger could never
//! have been told existed at all: an id from another sales channel or
//! another scope, exercised in `api_store.rs`. Which one is asserted below
//! follows the same rule `tests/api_permissions.rs` documents: ask before
//! answering `not_found`, so a miss and someone else's row read the same to
//! whoever asked.

mod common;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::Shop;
use rust_decimal_macros::dec;
use tezgah::api::agreement::{AcceptAgreement, PublishAgreement};
use tezgah::api::credit as api_credit;
use tezgah::api::credit::{ApplyGiftCard, ApplyStoreCredit};
use tezgah::api::digital;
use tezgah::api::store::{
    self, AddLineItem, CalculateShipping, ChooseShippingMethod, CreateCart, DeliveryInput,
    ListShippingOptions, RequestReturn, RequestTransfer, ReturnLineInput, StartPayment,
    StartPaymentSession, UpdateCart, UpdateLineItem, WriteAddress,
};
use tezgah::api::subscription as api_subscription;
use tezgah::api::{Method, Route, Surface, routes};
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::credit;
use tezgah::fulfilment;
use tezgah::id::{
    AddressId, CartId, CustomerId, OrderEntitlementId, OrderId, PaymentCollectionId, SellingPlanId,
    SubscriptionId,
};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, Host, JobSpec,
    Jobs, Permit, Resource, Tx,
};
use tezgah::pricing::{self, NewPrice};
use tezgah::subscription;

// ---------------------------------------------------------------------------
// A host that answers ownership honestly, and says yes to everything a
// question was never asked about.
// ---------------------------------------------------------------------------

/// The same idea as `api_store.rs`'s `OnlyOwner`, widened to the resources
/// this file's routes actually reach: a subscription and a saved address
/// (`Resource::Customer`, which is what `store::my_address` asks about)
/// besides the cart, order and payment it already covered.
#[derive(Debug, Default)]
struct Owner;

impl Authorizer for Owner {
    fn authorize(&self, actor: &Actor, _: Action, resource: &Resource) -> tezgah::Result<Permit> {
        let owner = match resource {
            Resource::Cart { customer, .. }
            | Resource::Order { customer, .. }
            | Resource::Payment { customer, .. }
            | Resource::Subscription { customer, .. } => *customer,
            Resource::Customer { id } => *id,
            _ => None,
        };

        match (owner, actor) {
            (None, _) => Ok(Permit::granted()),
            (Some(owner), Actor::Customer { id }) if owner == *id => Ok(Permit::granted()),
            (Some(_), _) => Err(tezgah::Error::denied()),
        }
    }
}

impl Clock for Owner {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for Owner {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for Owner {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for Owner {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn draft(handle: &str) -> NewProduct {
    NewProduct {
        handle: handle.into(),
        title: format!("A {handle}"),
        ..NewProduct::default()
    }
}

async fn a_customer(tx: &mut Tx<'_>, shop: &Shop, email: &str) -> tezgah::Result<CustomerId> {
    Ok(tezgah::customer::create(
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
    .await?
    .id)
}

/// A published, unchanneled hundred-lira variant — #109's backward-compatible
/// state, addable from any cart, so the fixture does not also have to build a
/// sales channel to be reachable.
async fn a_priced_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
) -> tezgah::Result<tezgah::id::VariantId> {
    let made = catalogue::create_product(tx, ctx, draft(handle)).await?;
    catalogue::publish_product(tx, ctx, made.id).await?;
    let variant = catalogue::create_variant(
        tx,
        ctx,
        made.id,
        NewVariant {
            title: "One size".into(),
            ..NewVariant::default()
        },
    )
    .await?;

    let set = pricing::create_price_set(tx, ctx).await?;
    pricing::link_variant(tx, ctx, variant.id, set.id).await?;
    pricing::add_price(
        tx,
        ctx,
        NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(dec!(100), Currency::parse("TRY")?),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await?;

    Ok(variant.id)
}

/// Everything one shopper owns: a cart with a line in it, an order, a saved
/// address, a payment collection opened on the cart, a subscription and a
/// downloadable entitlement — one of each kind of thing this file's routes
/// take an id for.
struct Shopper {
    id: CustomerId,
    cart_id: CartId,
    order_id: OrderId,
    order_line_id: tezgah::id::LineItemId,
    address_id: AddressId,
    collection_id: PaymentCollectionId,
    subscription_id: SubscriptionId,
    entitlement_id: OrderEntitlementId,
}

async fn a_shopper(
    tx: &mut Tx<'_>,
    shop: &Shop,
    host: &Owner,
    email: &str,
    variant: tezgah::id::VariantId,
    plan: SellingPlanId,
    content_id: uuid::Uuid,
) -> tezgah::Result<Shopper> {
    let id = a_customer(tx, shop, email).await?;
    let ctx = shop.ctx_as(Actor::Customer { id: id.as_uuid() }, host as &dyn Host);
    let token = tezgah::store::create_publishable_key(tx, &ctx, "storefront")
        .await?
        .token;

    let cart = store::create_cart(
        tx,
        &ctx,
        &token,
        CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await?;
    store::add_line_item(
        tx,
        &ctx,
        cart.id,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
            selling_plan_id: None,
        },
    )
    .await?;

    let placed = order::create(
        tx,
        &ctx,
        NewOrder {
            customer_id: Some(id),
            email: Some(email.into()),
            lines: vec![NewOrderLine::of(
                "A thing",
                1,
                Money::new(dec!(10), Currency::parse("TRY")?),
            )],
            ..NewOrder::of(Currency::parse("TRY")?)
        },
    )
    .await?;

    let address = store::add_my_address(
        tx,
        &ctx,
        WriteAddress {
            country_code: Some("TR".into()),
            ..WriteAddress::default()
        },
    )
    .await?;

    let collection =
        store::create_payment_collection(tx, &ctx, StartPayment { cart_id: cart.id }).await?;

    let contract = subscription::create(
        tx,
        &ctx,
        subscription::NewSubscription {
            customer_id: id,
            selling_plan_id: plan,
            currency: Currency::parse("TRY")?,
            region_id: None,
            sales_channel_id: None,
            account_holder_id: None,
            payment_method_reference: None,
            mandate_reference: None,
            mandate_accepted_at: None,
            shipping_address_id: None,
            billing_address_id: None,
            starts_at: None,
            lines: vec![subscription::NewLine {
                variant_id: variant,
                title: None,
                quantity: 1,
                unit_price: Money::new(dec!(100), Currency::parse("TRY")?),
            }],
        },
    )
    .await?;

    let order_line_id: uuid::Uuid = sqlx::query_scalar(
        "select id from order_line_item where scope = $1 and order_id = $2 limit 1",
    )
    .bind(shop.here.0)
    .bind(placed.id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    // The grant, written directly: going through `digital::grant` would need
    // a captured payment on this order, which is a whole checkout this file
    // does not otherwise need. Nothing here tests the granting path itself —
    // only that a granted entitlement's id is not another shopper's to reach.
    // The content itself is shared with the other shopper on purpose: the
    // same download sold twice, each buyer with their own entitlement, so a
    // refusal below proves ownership is asked about the entitlement rather
    // than inferred from the content being one shopper's alone.
    let entitlement_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into order_entitlement
             (id, scope, order_id, order_line_item_id, digital_content_id, customer_id,
              content_key)
         values ($1, $2, $3, $4, $5, $6, 'file.pdf')",
    )
    .bind(entitlement_id)
    .bind(shop.here.0)
    .bind(placed.id.as_uuid())
    .bind(order_line_id)
    .bind(content_id)
    .bind(id.as_uuid())
    .execute(&mut **tx)
    .await?;

    Ok(Shopper {
        id,
        cart_id: cart.id,
        order_id: placed.id,
        order_line_id: tezgah::id::LineItemId::from_uuid(order_line_id),
        address_id: address.id,
        collection_id: collection.id,
        subscription_id: contract.id,
        entitlement_id: OrderEntitlementId::from_uuid(entitlement_id),
    })
}

// ---------------------------------------------------------------------------
// Routes with no id to probe: a shopper's own account, listed or read back
// without ever naming anybody else's. Cannot leak by id because none is
// taken — there is nothing here for the matrix below to call twice.
// ---------------------------------------------------------------------------
static SELF_ONLY: &[(Method, &str)] = &[
    (Method::Post, "/store/customers"),
    (Method::Get, "/store/customers/me"),
    (Method::Post, "/store/customers/me"),
    (Method::Get, "/store/customers/me/addresses"),
    (Method::Post, "/store/customers/me/addresses"),
    (Method::Post, "/store/customers/me/account-holders"),
    (Method::Get, "/store/customers/me/store-credit"),
    (Method::Get, "/store/orders"),
    (Method::Get, "/store/subscriptions"),
    (Method::Get, "/store/entitlements"),
    (Method::Post, "/store/carts"),
];

/// Catalogue and shop-configuration reads: nothing here is scoped to a
/// customer at all, and the channel axis (#132, #135) is `api_store.rs`'s.
static PUBLIC: &[(Method, &str)] = &[
    (Method::Get, "/store/products"),
    (Method::Get, "/store/products/{handle}"),
    (Method::Get, "/store/product-variants"),
    (Method::Get, "/store/product-variants/{id}"),
    (Method::Get, "/store/product-options"),
    (Method::Get, "/store/product-options/{id}"),
    (Method::Get, "/store/product-tags"),
    (Method::Get, "/store/product-tags/{id}"),
    (Method::Get, "/store/product-types"),
    (Method::Get, "/store/product-types/{id}"),
    (Method::Get, "/store/product-categories"),
    (Method::Get, "/store/product-categories/{id}"),
    (Method::Get, "/store/collections"),
    (Method::Get, "/store/collections/{id}"),
    (Method::Get, "/store/regions"),
    (Method::Get, "/store/regions/{id}"),
    (Method::Get, "/store/currencies"),
    (Method::Get, "/store/currencies/{code}"),
    (Method::Get, "/store/locales"),
    (Method::Get, "/store/return-reasons"),
    (Method::Get, "/store/return-reasons/{id}"),
    (Method::Get, "/store/payment-providers"),
];

/// Not owner-checked by id at all, and each for a reason of its own:
static TOLERATED: &[(Method, &str, &str)] = &[
    // Needs a live PaymentProvider fixture; `tests/api_permissions.rs` already
    // tolerates it for the same reason.
    (
        Method::Post,
        "/store/carts/{id}/complete",
        "needs a PaymentProvider fixture this test does not build",
    ),
    // Gated by a possession token, not a customer id — the order in the path
    // is somebody else's until the token says otherwise. Exercised below for
    // "the wrong token refuses", which is the question this route actually
    // answers.
    (
        Method::Post,
        "/store/orders/{id}/transfer/accept",
        "ownership is the token's to prove, not the id's; see the token check below",
    ),
    (
        Method::Post,
        "/store/orders/{id}/transfer/decline",
        "ownership is the token's to prove, not the id's; see the token check below",
    ),
    // Spends a download token, not an entitlement id — the same shape as the
    // transfer routes above.
    (
        Method::Post,
        "/store/downloads",
        "ownership is the token's to prove, not an id in the path or body",
    ),
];

fn tolerated(method: Method, path: &str) -> bool {
    TOLERATED.iter().any(|(m, p, _)| *m == method && *p == path)
        || SELF_ONLY.iter().any(|(m, p)| *m == method && *p == path)
        || PUBLIC.iter().any(|(m, p)| *m == method && *p == path)
}

#[tokio::test]
async fn every_owned_store_route_refuses_the_other_shoppers_resource() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let host = Owner;
    let mut tx = shop.begin().await;

    common::a_currency(&mut tx, shop.here, "TRY", 2).await;
    tezgah::payment::register_provider(&mut tx, &shop.ctx(), "mock").await?;

    let variant = a_priced_variant(&mut tx, &shop.ctx(), "isolation-thing").await?;
    let group = subscription::create_plan_group(
        &mut tx,
        &shop.ctx(),
        subscription::NewPlanGroup {
            name: "Monthly".into(),
            ..subscription::NewPlanGroup::default()
        },
    )
    .await?;
    let plan = subscription::create_plan(
        &mut tx,
        &shop.ctx(),
        group.id,
        subscription::NewPlan {
            name: "Standard".into(),
            billing_interval_unit: "month".into(),
            billing_interval_count: 1,
            ..subscription::NewPlan::default()
        },
    )
    .await?;
    subscription::attach_variant(&mut tx, &shop.ctx(), plan.id, variant).await?;

    let published = tezgah::api::agreement::publish_agreement(
        &mut tx,
        &shop.ctx(),
        PublishAgreement {
            kind: "distance_sale".into(),
            locale: "en".into(),
            body: "Terms.".into(),
            effective_from: None,
            metadata: None,
        },
    )
    .await?;

    // Sold to both shoppers below: the same title, two separate entitlements,
    // so the matrix proves ownership is asked about the entitlement rather
    // than about who happens to hold the only copy.
    let content_id = uuid::Uuid::now_v7();
    sqlx::query(
        "insert into digital_content
             (id, scope, variant_id, content_key, name, auto_grant, rank)
         values ($1, $2, $3, 'file.pdf', 'A download', true, 0)",
    )
    .bind(content_id)
    .bind(shop.here.0)
    .bind(variant.as_uuid())
    .execute(&mut *tx)
    .await?;

    let a = a_shopper(
        &mut tx,
        &shop,
        &host,
        "a@example.test",
        variant,
        plan.id,
        content_id,
    )
    .await?;
    let b = a_shopper(
        &mut tx,
        &shop,
        &host,
        "b@example.test",
        variant,
        plan.id,
        content_id,
    )
    .await?;

    let ctx_a = shop.ctx_as(Actor::Customer { id: a.id.as_uuid() }, &host as &dyn Host);
    let ctx_b = shop.ctx_as(Actor::Customer { id: b.id.as_uuid() }, &host as &dyn Host);

    let mut covered: Vec<(Method, &'static str)> = Vec::new();
    let mut leaks: Vec<String> = Vec::new();
    let mut refused_mine: Vec<String> = Vec::new();

    /// A owns it and reaches it; B does not own it and is refused, by the
    /// error kind `$kind` says (`denied` or `not_found`).
    macro_rules! owned {
        ($method:expr, $path:literal, $kind:ident, mine: $mine:expr, theirs: $theirs:expr) => {{
            covered.push(($method, $path));
            match $mine.await {
                Ok(_) => {}
                Err(error) => refused_mine.push(format!(
                    "{} {}: A was refused its own resource: {error}",
                    stringify!($method),
                    $path
                )),
            }
            match $theirs.await {
                Ok(_) => leaks.push(format!(
                    "{} {}: B reached A's resource",
                    stringify!($method),
                    $path
                )),
                Err(error) if error.$kind() => {}
                Err(error) => leaks.push(format!(
                    "{} {}: refused, but as {:?} rather than {}",
                    stringify!($method),
                    $path,
                    error.code(),
                    stringify!($kind)
                )),
            }
        }};
    }

    // ------------------------------------------------------------- cart ---
    owned!(
        Method::Get,
        "/store/carts/{id}",
        is_denied,
        mine: store::get_cart(&mut tx, &ctx_a, a.cart_id),
        theirs: store::get_cart(&mut tx, &ctx_b, a.cart_id)
    );
    owned!(
        Method::Post,
        "/store/carts/{id}",
        is_denied,
        mine: store::update_cart(&mut tx, &ctx_a, a.cart_id, UpdateCart::default()),
        theirs: store::update_cart(&mut tx, &ctx_b, a.cart_id, UpdateCart::default())
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/customer",
        is_denied,
        // A's cart is already hers; handing it to its own owner again is a
        // no-op, and still runs `own_cart` first, which is what is on trial.
        mine: store::set_cart_customer(&mut tx, &ctx_a, a.cart_id),
        theirs: store::set_cart_customer(&mut tx, &ctx_b, a.cart_id)
    );
    owned!(
        Method::Get,
        "/store/carts/{id}/line-items",
        is_denied,
        mine: store::list_line_items(&mut tx, &ctx_a, a.cart_id),
        theirs: store::list_line_items(&mut tx, &ctx_b, a.cart_id)
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/line-items",
        is_denied,
        mine: store::add_line_item(
            &mut tx,
            &ctx_a,
            a.cart_id,
            AddLineItem {
                variant_id: variant,
                quantity: 1,
                selling_plan_id: None
            }
        ),
        theirs: store::add_line_item(
            &mut tx,
            &ctx_b,
            a.cart_id,
            AddLineItem {
                variant_id: variant,
                quantity: 1,
                selling_plan_id: None
            }
        )
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/bundle-items",
        is_denied,
        // Not a bundle, so "mine" refuses too, but on `invalid` rather than an
        // ownership error — it clears `own_cart` first either way, which is
        // the only thing this route is on trial for here.
        mine: async {
            match store::add_bundle_item(
                &mut tx,
                &ctx_a,
                a.cart_id,
                store::AddBundleItem {
                    variant_id: variant,
                    quantity: 1
                }
            )
            .await
            {
                Ok(_) => Ok(()),
                Err(error) if error.code() == "invalid" => Ok(()),
                Err(error) => Err(error),
            }
        },
        theirs: store::add_bundle_item(
            &mut tx,
            &ctx_b,
            a.cart_id,
            store::AddBundleItem {
                variant_id: variant,
                quantity: 1
            }
        )
    );
    let a_line = store::list_line_items(&mut tx, &ctx_a, a.cart_id)
        .await?
        .into_iter()
        .next()
        .expect("A's cart has the line it was seeded with")
        .id;
    owned!(
        Method::Post,
        "/store/carts/{id}/line-items/{line_id}",
        is_denied,
        mine: store::update_line_item(&mut tx, &ctx_a, a.cart_id, a_line, UpdateLineItem { quantity: 2 }),
        theirs: store::update_line_item(&mut tx, &ctx_b, a.cart_id, a_line, UpdateLineItem { quantity: 2 })
    );
    // A second line, just for the delete below: removing `a_line` here would
    // leave later routes that still expect a priced line on this cart with
    // none.
    let doomed_line = store::add_line_item(
        &mut tx,
        &ctx_a,
        a.cart_id,
        AddLineItem {
            variant_id: variant,
            quantity: 1,
            selling_plan_id: Some(plan.id),
        },
    )
    .await?
    .id;
    owned!(
        Method::Delete,
        "/store/carts/{id}/line-items/{line_id}",
        is_denied,
        mine: store::remove_line_item(&mut tx, &ctx_a, a.cart_id, doomed_line),
        theirs: store::remove_line_item(&mut tx, &ctx_b, a.cart_id, doomed_line)
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/promotions",
        is_denied,
        mine: store::apply_promotions(&mut tx, &ctx_a, a.cart_id),
        theirs: store::apply_promotions(&mut tx, &ctx_b, a.cart_id)
    );
    let set = fulfilment::create_set(
        &mut tx,
        &shop.ctx(),
        fulfilment::NewFulfillmentSet {
            name: "Shipping".into(),
            kind: fulfilment::SetKind::Shipping,
        },
    )
    .await?;
    let zone = fulfilment::create_service_zone(
        &mut tx,
        &shop.ctx(),
        fulfilment::NewServiceZone {
            name: "Everywhere".into(),
            fulfillment_set_id: set.id,
        },
    )
    .await?;
    let profile_id =
        fulfilment::create_shipping_profile(&mut tx, &shop.ctx(), "Standard", "default").await?;
    let option = fulfilment::create_shipping_option(
        &mut tx,
        &shop.ctx(),
        fulfilment::NewShippingOption {
            name: "Courier".into(),
            service_zone_id: zone.id,
            shipping_profile_id: Some(profile_id),
            price_type: fulfilment::PriceKind::Flat,
            provider_id: None,
            data: None,
        },
    )
    .await?;
    owned!(
        Method::Post,
        "/store/carts/{id}/shipping-methods",
        is_denied,
        mine: async {
            match store::set_shipping_method(
                &mut tx,
                &ctx_a,
                a.cart_id,
                ChooseShippingMethod {
                    shipping_option_id: option.id
                }
            )
            .await
            {
                Ok(_) => Ok(()),
                // No price attached in this currency is a fine outcome here:
                // `own_cart` has already run, which is what this route is on
                // trial for.
                Err(error) if error.code() == "invalid" => Ok(()),
                Err(error) => Err(error),
            }
        },
        theirs: store::set_shipping_method(
            &mut tx,
            &ctx_b,
            a.cart_id,
            ChooseShippingMethod {
                shipping_option_id: option.id
            }
        )
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/taxes",
        is_denied,
        mine: store::quote_taxes(
            &mut tx,
            &ctx_a,
            a.cart_id,
            DeliveryInput {
                country_code: "TR".into(),
                province_code: None,
                city: None,
                postal_code: None,
                evidence: Vec::new(),
            }
        ),
        theirs: store::quote_taxes(
            &mut tx,
            &ctx_b,
            a.cart_id,
            DeliveryInput {
                country_code: "TR".into(),
                province_code: None,
                city: None,
                postal_code: None,
                evidence: Vec::new(),
            }
        )
    );
    owned!(
        Method::Post,
        "/store/carts/{id}/tax-evidence",
        is_denied,
        mine: store::reprice_with_evidence(&mut tx, &ctx_a, a.cart_id, Vec::new()),
        theirs: store::reprice_with_evidence(&mut tx, &ctx_b, a.cart_id, Vec::new())
    );
    owned!(
        Method::Get,
        "/store/shipping-options",
        is_denied,
        mine: store::list_shipping_options(
            &mut tx,
            &ctx_a,
            ListShippingOptions {
                cart_id: a.cart_id,
                country_code: "TR".into(),
                province_code: None,
                city: None,
                postal_code: None,
            }
        ),
        theirs: store::list_shipping_options(
            &mut tx,
            &ctx_b,
            ListShippingOptions {
                cart_id: a.cart_id,
                country_code: "TR".into(),
                province_code: None,
                city: None,
                postal_code: None,
            }
        )
    );
    owned!(
        Method::Post,
        "/store/shipping-options/{id}/calculate",
        is_denied,
        mine: async {
            match store::calculate_shipping_option(
                &mut tx,
                &ctx_a,
                option.id,
                CalculateShipping { cart_id: a.cart_id }
            )
            .await
            {
                Ok(_) => Ok(()),
                Err(error) if error.is_not_found() => Ok(()),
                Err(error) => Err(error),
            }
        },
        theirs: store::calculate_shipping_option(
            &mut tx,
            &ctx_b,
            option.id,
            CalculateShipping { cart_id: a.cart_id }
        )
    );
    let card = credit::issue(
        &mut tx,
        &ctx_a,
        credit::NewGiftCard {
            balance: Money::new(dec!(50), Currency::parse("TRY")?),
            issued_order_id: None,
            customer_id: None,
            expires_at: None,
            reason: None,
            line: None,
        },
    )
    .await?;
    owned!(
        Method::Post,
        "/store/carts/{id}/gift-cards",
        is_denied,
        mine: api_credit::apply_gift_card(
            &mut tx,
            &ctx_a,
            a.cart_id,
            ApplyGiftCard {
                code: card.code.clone(),
                amount: tezgah::api::credit::AmountIn {
                    amount: dec!(10),
                    currency_code: "TRY".into(),
                },
            }
        ),
        theirs: api_credit::apply_gift_card(
            &mut tx,
            &ctx_b,
            a.cart_id,
            ApplyGiftCard {
                code: card.code.clone(),
                amount: tezgah::api::credit::AmountIn {
                    amount: dec!(10),
                    currency_code: "TRY".into(),
                },
            }
        )
    );
    credit::grant_store_credit(
        &mut tx,
        &ctx_a,
        a.id,
        Money::new(dec!(50), Currency::parse("TRY")?),
        None,
    )
    .await?;
    owned!(
        Method::Post,
        "/store/carts/{id}/store-credit",
        is_denied,
        mine: api_credit::apply_store_credit(
            &mut tx,
            &ctx_a,
            a.cart_id,
            ApplyStoreCredit {
                amount: tezgah::api::credit::AmountIn {
                    amount: dec!(10),
                    currency_code: "TRY".into(),
                }
            }
        ),
        theirs: api_credit::apply_store_credit(
            &mut tx,
            &ctx_b,
            a.cart_id,
            ApplyStoreCredit {
                amount: tezgah::api::credit::AmountIn {
                    amount: dec!(10),
                    currency_code: "TRY".into(),
                }
            }
        )
    );
    owned!(
        Method::Get,
        "/store/carts/{id}/credits",
        is_denied,
        mine: api_credit::list_cart_credits(&mut tx, &ctx_a, a.cart_id),
        theirs: api_credit::list_cart_credits(&mut tx, &ctx_b, a.cart_id)
    );
    let a_credit = api_credit::list_cart_credits(&mut tx, &ctx_a, a.cart_id)
        .await?
        .into_iter()
        .next()
        .expect("the gift card just applied");
    owned!(
        Method::Delete,
        "/store/carts/{id}/credits/{credit_id}",
        is_denied,
        mine: api_credit::remove_cart_credit(&mut tx, &ctx_a, a.cart_id, a_credit.id),
        theirs: api_credit::remove_cart_credit(&mut tx, &ctx_b, a.cart_id, a_credit.id)
    );

    // ------------------------------------------------------ addresses ----
    owned!(
        Method::Post,
        "/store/customers/me/addresses/{address_id}",
        is_denied,
        mine: store::update_my_address(&mut tx, &ctx_a, a.address_id, WriteAddress::default()),
        theirs: store::update_my_address(&mut tx, &ctx_b, a.address_id, WriteAddress::default())
    );
    // Deleting is done last, on the other end of the run, so the row is
    // still there for `theirs` to be refused against; "mine" is proven above.

    // -------------------------------------------------------- payment ----
    owned!(
        Method::Post,
        "/store/payment-collections",
        is_denied,
        mine: store::create_payment_collection(&mut tx, &ctx_a, StartPayment { cart_id: a.cart_id }),
        theirs: store::create_payment_collection(&mut tx, &ctx_b, StartPayment { cart_id: a.cart_id })
    );
    owned!(
        Method::Post,
        "/store/payment-collections/{id}/payment-sessions",
        is_denied,
        mine: store::create_payment_session(
            &mut tx,
            &ctx_a,
            a.collection_id,
            StartPaymentSession {
                cart_id: a.cart_id,
                provider_code: "mock".into(),
                context: None,
            }
        ),
        // B names its own cart — the only one it may open a session
        // against — and A's collection in the path. `payment::collection`
        // reads the collection's real owner and asks about it before
        // `create_payment_session` ever gets to compare it with the cart in
        // the body, so this is refused as `denied` rather than reaching
        // #82's own `cart_id` mismatch check at all.
        theirs: store::create_payment_session(
            &mut tx,
            &ctx_b,
            a.collection_id,
            StartPaymentSession {
                cart_id: b.cart_id,
                provider_code: "mock".into(),
                context: None,
            }
        )
    );

    // ----------------------------------------------------------- order ---
    owned!(
        Method::Get,
        "/store/orders/{id}",
        is_denied,
        mine: store::get_my_order(&mut tx, &ctx_a, a.order_id),
        theirs: store::get_my_order(&mut tx, &ctx_b, a.order_id)
    );
    owned!(
        Method::Post,
        "/store/orders/{id}/transfer/request",
        is_denied,
        mine: store::request_transfer(
            &mut tx,
            &ctx_a,
            a.order_id,
            RequestTransfer {
                to_email: "somebody-else@example.test".into(),
                expires_at: shop.ctx().now() + chrono::Duration::days(1),
            }
        ),
        theirs: store::request_transfer(
            &mut tx,
            &ctx_b,
            a.order_id,
            RequestTransfer {
                to_email: "somebody-else@example.test".into(),
                expires_at: shop.ctx().now() + chrono::Duration::days(1),
            }
        )
    );
    owned!(
        Method::Post,
        "/store/orders/{id}/transfer/cancel",
        is_denied,
        mine: store::cancel_transfer(&mut tx, &ctx_a, a.order_id),
        theirs: store::cancel_transfer(&mut tx, &ctx_b, a.order_id)
    );
    owned!(
        Method::Post,
        "/store/returns",
        is_denied,
        // A real line, not a fabricated id: a return writes rows with a
        // genuine foreign key to it, and a made-up id would abort the whole
        // transaction on a constraint violation rather than answer with a
        // catchable error — the wrong way to prove `own_order` ran first.
        // B's call never reaches that far, since it is refused before the
        // line is ever touched.
        mine: store::request_return(
            &mut tx,
            &ctx_a,
            RequestReturn {
                order_id: a.order_id,
                lines: vec![ReturnLineInput {
                    order_line_item_id: a.order_line_id,
                    quantity: 1,
                    return_reason_id: None,
                    note: None,
                }],
            }
        ),
        theirs: store::request_return(
            &mut tx,
            &ctx_b,
            RequestReturn {
                order_id: a.order_id,
                lines: vec![ReturnLineInput {
                    order_line_item_id: a.order_line_id,
                    quantity: 1,
                    return_reason_id: None,
                    note: None,
                }],
            }
        )
    );
    owned!(
        Method::Post,
        "/store/orders/{id}/agreements",
        is_denied,
        mine: tezgah::api::agreement::accept_agreement(
            &mut tx,
            &ctx_a,
            a.order_id,
            AcceptAgreement {
                agreement_version_id: published.id,
                accepted_at: None,
                ip: None,
                user_agent: None,
                metadata: None,
            }
        ),
        theirs: tezgah::api::agreement::accept_agreement(
            &mut tx,
            &ctx_b,
            a.order_id,
            AcceptAgreement {
                agreement_version_id: published.id,
                accepted_at: None,
                ip: None,
                user_agent: None,
                metadata: None,
            }
        )
    );
    owned!(
        Method::Get,
        "/store/orders/{id}/agreements/{kind}",
        is_denied,
        mine: tezgah::api::agreement::my_accepted_text(&mut tx, &ctx_a, a.order_id, "distance_sale"),
        theirs: tezgah::api::agreement::my_accepted_text(&mut tx, &ctx_b, a.order_id, "distance_sale")
    );

    // ------------------------------------------------------ subscription -
    owned!(
        Method::Post,
        "/store/subscriptions/{id}/pause",
        is_denied,
        mine: api_subscription::pause_my_subscription(&mut tx, &ctx_a, a.subscription_id, api_subscription::Pause { until: None }),
        theirs: api_subscription::pause_my_subscription(&mut tx, &ctx_b, a.subscription_id, api_subscription::Pause { until: None })
    );
    owned!(
        Method::Post,
        "/store/subscriptions/{id}/resume",
        is_denied,
        mine: api_subscription::resume_my_subscription(&mut tx, &ctx_a, a.subscription_id),
        theirs: api_subscription::resume_my_subscription(&mut tx, &ctx_b, a.subscription_id)
    );
    owned!(
        Method::Post,
        "/store/subscriptions/{id}/skip",
        is_denied,
        mine: api_subscription::skip_my_subscription(&mut tx, &ctx_a, a.subscription_id),
        theirs: api_subscription::skip_my_subscription(&mut tx, &ctx_b, a.subscription_id)
    );
    // Cancelling is last for the same reason deleting an address is: every
    // other subscription route above still needs a live contract to run
    // against.
    owned!(
        Method::Post,
        "/store/subscriptions/{id}/cancel",
        is_denied,
        mine: api_subscription::cancel_my_subscription(&mut tx, &ctx_a, a.subscription_id, api_subscription::Cancel { immediately: None, reason: None }),
        theirs: api_subscription::cancel_my_subscription(&mut tx, &ctx_b, a.subscription_id, api_subscription::Cancel { immediately: None, reason: None })
    );

    // ------------------------------------------------------- entitlement -
    owned!(
        Method::Post,
        "/store/entitlements/{id}/token",
        is_denied,
        mine: digital::create_token(&mut tx, &ctx_a, a.entitlement_id),
        theirs: digital::create_token(&mut tx, &ctx_b, a.entitlement_id)
    );

    // Now that every other cart route has run, the address and the
    // subscription cancelled above, delete the address a route earlier only
    // updated.
    owned!(
        Method::Delete,
        "/store/customers/me/addresses/{address_id}",
        is_denied,
        mine: async { Ok::<(), tezgah::Error>(()) },
        theirs: store::delete_my_address(&mut tx, &ctx_b, a.address_id)
    );
    store::delete_my_address(&mut tx, &ctx_a, a.address_id).await?;

    // ----------------------------------------------------- token-gated ---
    // Not customer-id checks — see `TOLERATED` — but still exercised: the
    // wrong token refuses exactly as the id-based routes above do, just on a
    // different fact.
    let garbage_token = "not-a-real-transfer-token".to_owned();
    assert!(
        store::accept_transfer(
            &mut tx,
            &ctx_b,
            a.order_id,
            tezgah::api::store::ClaimTransfer {
                token: garbage_token.clone()
            }
        )
        .await
        .is_err(),
        "a made-up transfer token does not hand B someone else's order"
    );
    assert!(
        store::decline_transfer(
            &mut tx,
            &ctx_b,
            a.order_id,
            tezgah::api::store::ClaimTransfer {
                token: garbage_token
            }
        )
        .await
        .is_err(),
        "nor does it let B decline an offer that was never made to them"
    );
    assert!(
        digital::redeem(
            &mut tx,
            &ctx_b,
            digital::Redeem {
                token: "not-a-real-download-token".into(),
                ip: None,
                user_agent: None,
            }
        )
        .await
        .is_err(),
        "a made-up download token does not spend A's entitlement"
    );

    assert!(
        leaks.is_empty(),
        "B reached something of A's:\n{}",
        leaks.join("\n")
    );
    assert!(
        refused_mine.is_empty(),
        "A was refused its own resource, so the matrix proves nothing about \
         these routes' happy path:\n{}",
        refused_mine.join("\n")
    );

    // ---------------------------------------------------- completeness ---
    let store_routes: Vec<Route> = routes()
        .into_iter()
        .filter(|route| route.surface == Surface::Store)
        .collect();

    let mut missing: Vec<String> = Vec::new();
    for route in &store_routes {
        let named =
            covered.contains(&(route.method, route.path)) || tolerated(route.method, route.path);
        if !named {
            missing.push(format!("{} {}", route.method.as_str(), route.path));
        }
    }
    assert!(
        missing.is_empty(),
        "a store route is neither tested for cross-tenant isolation nor \
         accounted for in SELF_ONLY, PUBLIC or TOLERATED:\n{}",
        missing.join("\n")
    );

    drop(tx);
    shop.close().await;
    Ok(())
}
