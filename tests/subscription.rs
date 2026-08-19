//! Billing the same contract again next month, and what happens when it fails.
//!
//! The claims worth holding here are all about time and repetition: the same
//! cycle is billed once however many schedulers ask for it, a price that moved
//! is charged at what it is now, a declined card ends in exactly one
//! cancellation, and a failure anywhere leaves the contract where it was so the
//! next poll continues rather than repeats.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::id::{
    AccountHolderId, CustomerId, LineItemId, PriceId, SellingPlanId, StockLocationId,
    SubscriptionId, VariantId,
};
use tezgah::money::{Currency, Money};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, CancelRequest, CaptureRequest,
    CaptureResult, PaymentProvider, RecurringProvider, RefundRequest, RefundResult, SessionRequest,
    SessionResponse, SessionStatus, StoredChargeRequest, WebhookEvent,
};
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, Host, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use tezgah::subscription::{self, NewLine, NewPlan, NewPlanGroup, NewSubscription, Renewals};
use tezgah::workflow::State;
use tezgah::{Paging, credit, inventory, pricing};
use uuid::Uuid;

fn try_() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

// ---------------------------------------------------------------------------
// Banks
// ---------------------------------------------------------------------------

/// A bank that charges whatever stored instrument it is shown.
#[derive(Debug, Default)]
struct Standing;

/// A bank that refuses every stored charge, which is the whole of dunning's
/// input.
#[derive(Debug, Default)]
struct Refusing;

/// A bank that wants the shopper back, off-session, where there is no shopper.
#[derive(Debug, Default)]
struct Insisting;

macro_rules! bank {
    ($name:ident) => {
        #[async_trait]
        impl PaymentProvider for $name {
            fn code(&self) -> &'static str {
                "bank"
            }

            async fn create_session(&self, _: SessionRequest) -> tezgah::Result<SessionResponse> {
                Ok(SessionResponse {
                    data: serde_json::json!({}),
                    status: SessionStatus::Pending,
                })
            }

            async fn authorize(&self, req: AuthorizeRequest) -> tezgah::Result<Authorization> {
                Ok(Authorization {
                    status: AuthorizationStatus::Authorized,
                    amount: Some(req.amount),
                    data: serde_json::json!({}),
                    redirect: None,
                    message: None,
                    installment: None,
                })
            }

            async fn capture(&self, req: CaptureRequest) -> tezgah::Result<CaptureResult> {
                Ok(CaptureResult {
                    amount: req.amount,
                    data: serde_json::Value::Null,
                })
            }

            async fn refund(&self, req: RefundRequest) -> tezgah::Result<RefundResult> {
                Ok(RefundResult {
                    amount: req.amount,
                    data: serde_json::Value::Null,
                })
            }

            async fn cancel(&self, _: CancelRequest) -> tezgah::Result<()> {
                Ok(())
            }

            fn parse_webhook(
                &self,
                _: &[(String, String)],
                _: &[u8],
            ) -> tezgah::Result<WebhookEvent> {
                Err(tezgah::Error::invalid("this bank sends no webhooks"))
            }
        }
    };
}

bank!(Standing);
bank!(Refusing);
bank!(Insisting);

#[async_trait]
impl RecurringProvider for Standing {
    async fn authorize_stored(&self, req: StoredChargeRequest) -> tezgah::Result<Authorization> {
        assert!(!req.account_holder.is_empty(), "no account holder was sent");
        assert!(
            !req.payment_method_reference.is_empty(),
            "no stored instrument was named"
        );

        Ok(Authorization {
            status: AuthorizationStatus::Authorized,
            amount: Some(req.amount),
            data: serde_json::json!({ "off_session": true }),
            redirect: None,
            message: None,
            installment: None,
        })
    }
}

#[async_trait]
impl RecurringProvider for Refusing {
    async fn authorize_stored(&self, _: StoredChargeRequest) -> tezgah::Result<Authorization> {
        Ok(Authorization {
            status: AuthorizationStatus::Error,
            amount: None,
            data: serde_json::json!({}),
            redirect: None,
            message: Some("the card was declined".into()),
            installment: None,
        })
    }
}

#[async_trait]
impl RecurringProvider for Insisting {
    async fn authorize_stored(&self, _: StoredChargeRequest) -> tezgah::Result<Authorization> {
        Ok(Authorization {
            status: AuthorizationStatus::RequiresMore,
            amount: None,
            data: serde_json::json!({}),
            redirect: Some("https://example.test/3ds".into()),
            message: None,
            installment: None,
        })
    }
}

// ---------------------------------------------------------------------------
// Hosts
// ---------------------------------------------------------------------------

/// A host that will not let the system do anything, which is what a renewal is.
#[derive(Debug, Default)]
struct NoSystem;

impl Authorizer for NoSystem {
    fn authorize(&self, actor: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        match actor {
            Actor::System => Err(tezgah::Error::denied()),
            _ => Ok(Permit::granted()),
        }
    }
}

/// A host whose job queue is broken. What it proves is that the status change
/// and the retry it belongs to are one transaction: if they were not, the
/// contract would be left `past_due` with nothing coming back for it.
#[derive(Debug, Default)]
struct NoJobs;

impl Authorizer for NoJobs {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Ok(Permit::granted())
    }
}

macro_rules! quiet_host {
    ($name:ident, $jobs:expr) => {
        impl Clock for $name {
            fn now(&self) -> DateTime<Utc> {
                Utc::now()
            }
        }

        #[async_trait]
        impl AuditSink for $name {
            async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl EventSink for $name {
            async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
                Ok(())
            }
        }

        #[async_trait]
        impl Jobs for $name {
            async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
                $jobs
            }
        }
    };
}

quiet_host!(NoSystem, Ok(()));
quiet_host!(
    NoJobs,
    Err(tezgah::Error::conflict("this queue is not accepting work"))
);

// ---------------------------------------------------------------------------
// A shop with one contract in it
// ---------------------------------------------------------------------------

struct Contract {
    id: SubscriptionId,
    plan_id: SellingPlanId,
    variant_id: VariantId,
    price_id: PriceId,
    location_id: StockLocationId,
    customer_id: CustomerId,
    holder_id: AccountHolderId,
}

/// A monthly contract for one thing at `price`, already a month overdue.
async fn a_contract(shop: &Shop, price: Decimal, max_cycles: Option<i32>) -> Contract {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, 'bank', true)
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a monthly thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 100, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::link_variant(&mut tx, &ctx, variant, set.id)
        .await
        .expect("the variant to be priced by it");
    let written = pricing::add_price(
        &mut tx,
        &ctx,
        pricing::NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(price, try_()),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await
    .expect("a price");

    let customer = common::a_customer(&mut tx, &ctx).await;

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into customer_address (id, scope, customer_id, address_1, city, country_code)
         values ($1, $2, $3, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(shop.here.0)
    .bind(customer.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("an address");

    let holder = payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: "bank".into(),
            customer_id: Some(customer),
            external_id: format!("cus_{}", Uuid::now_v7().simple()),
            email: None,
            data: serde_json::json!({}),
        },
    )
    .await
    .expect("an account holder");

    let group = subscription::create_plan_group(
        &mut tx,
        &ctx,
        NewPlanGroup {
            name: "Monthly".into(),
            ..NewPlanGroup::default()
        },
    )
    .await
    .expect("a group");

    let plan = subscription::create_plan(
        &mut tx,
        &ctx,
        group.id,
        NewPlan {
            name: "Every month".into(),
            billing_interval_unit: "month".into(),
            billing_interval_count: 1,
            max_cycles,
            ..NewPlan::default()
        },
    )
    .await
    .expect("a plan");

    subscription::attach_variant(&mut tx, &ctx, plan.id, variant)
        .await
        .expect("the variant to be sold on it");

    let contract = subscription::create(
        &mut tx,
        &ctx,
        NewSubscription {
            customer_id: customer,
            selling_plan_id: plan.id,
            currency: try_(),
            region_id: None,
            sales_channel_id: None,
            account_holder_id: Some(holder.id),
            payment_method_reference: Some("pm_a_saved_card".into()),
            mandate_reference: Some("mandate-1".into()),
            mandate_accepted_at: Some(Utc::now()),
            shipping_address_id: Some(tezgah::id::AddressId::from_uuid(address)),
            billing_address_id: None,
            // Two months back, so the first period ended a month ago and the
            // contract is owed a renewal the moment it exists.
            starts_at: Some(Utc::now() - Duration::days(60)),
            lines: vec![NewLine {
                variant_id: variant,
                title: Some("A thing, monthly".into()),
                quantity: 1,
                unit_price: Money::new(price, try_()),
            }],
        },
    )
    .await
    .expect("a contract");

    tx.commit().await.expect("to commit the seed");

    Contract {
        id: contract.id,
        plan_id: plan.id,
        variant_id: variant,
        price_id: written.id,
        location_id: location.id,
        customer_id: customer,
        holder_id: holder.id,
    }
}

/// The same seed as [`a_contract`], with the plan built from whatever `plan`
/// asks for rather than only its `max_cycles` — pause, skip, swap and prepaid
/// terms all need a knob `a_contract` does not expose.
async fn a_contract_with(shop: &Shop, price: Decimal, plan: NewPlan) -> Contract {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, 'bank', true)
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a monthly thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 100, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::link_variant(&mut tx, &ctx, variant, set.id)
        .await
        .expect("the variant to be priced by it");
    let written = pricing::add_price(
        &mut tx,
        &ctx,
        pricing::NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(price, try_()),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await
    .expect("a price");

    let customer = common::a_customer(&mut tx, &ctx).await;

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into customer_address (id, scope, customer_id, address_1, city, country_code)
         values ($1, $2, $3, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(shop.here.0)
    .bind(customer.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("an address");

    let holder = payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: "bank".into(),
            customer_id: Some(customer),
            external_id: format!("cus_{}", Uuid::now_v7().simple()),
            email: None,
            data: serde_json::json!({}),
        },
    )
    .await
    .expect("an account holder");

    let group = subscription::create_plan_group(
        &mut tx,
        &ctx,
        NewPlanGroup {
            name: "Monthly".into(),
            ..NewPlanGroup::default()
        },
    )
    .await
    .expect("a group");

    let plan = subscription::create_plan(&mut tx, &ctx, group.id, plan)
        .await
        .expect("a plan");

    subscription::attach_variant(&mut tx, &ctx, plan.id, variant)
        .await
        .expect("the variant to be sold on it");

    let contract = subscription::create(
        &mut tx,
        &ctx,
        NewSubscription {
            customer_id: customer,
            selling_plan_id: plan.id,
            currency: try_(),
            region_id: None,
            sales_channel_id: None,
            account_holder_id: Some(holder.id),
            payment_method_reference: Some("pm_a_saved_card".into()),
            mandate_reference: Some("mandate-1".into()),
            mandate_accepted_at: Some(Utc::now()),
            shipping_address_id: Some(tezgah::id::AddressId::from_uuid(address)),
            billing_address_id: None,
            starts_at: Some(Utc::now()),
            lines: vec![NewLine {
                variant_id: variant,
                title: Some("A thing, monthly".into()),
                quantity: 1,
                unit_price: Money::new(price, try_()),
            }],
        },
    )
    .await
    .expect("a contract");

    tx.commit().await.expect("to commit the seed");

    Contract {
        id: contract.id,
        plan_id: plan.id,
        variant_id: variant,
        price_id: written.id,
        location_id: location.id,
        customer_id: customer,
        holder_id: holder.id,
    }
}

/// A second variant, sellable on the same plan a contract already exists
/// under — what [`subscription::swap`] moves a contract onto.
async fn a_second_variant(shop: &Shop, plan_id: SellingPlanId, price: Decimal) -> VariantId {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("Another thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The bigger one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::link_variant(&mut tx, &ctx, variant, set.id)
        .await
        .expect("the variant to be priced by it");
    pricing::add_price(
        &mut tx,
        &ctx,
        pricing::NewPrice {
            price_set_id: set.id,
            price_list_id: None,
            title: None,
            amount: Money::new(price, try_()),
            min_quantity: None,
            max_quantity: None,
            rules: Vec::new(),
        },
    )
    .await
    .expect("a price");

    subscription::attach_variant(&mut tx, &ctx, plan_id, variant)
        .await
        .expect("the variant to be sold on the plan");

    tx.commit().await.expect("to commit");
    variant
}

async fn orders(shop: &Shop) -> i64 {
    let mut tx = shop.begin().await;
    let count: i64 = sqlx::query_scalar(r#"select count(*) from "order" where scope = $1"#)
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("to count the orders");
    tx.commit().await.expect("to commit");
    count
}

async fn billed_cycles(shop: &Shop, id: SubscriptionId) -> Vec<i32> {
    let mut tx = shop.begin().await;
    let cycles: Vec<i32> = sqlx::query_scalar(
        "select cycle from subscription_order
         where scope = $1 and subscription_id = $2 order by cycle",
    )
    .bind(shop.here.0)
    .bind(id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("to read the billed cycles");
    tx.commit().await.expect("to commit");
    cycles
}

async fn read(shop: &Shop, id: SubscriptionId) -> subscription::Subscription {
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let found = subscription::get(&mut tx, &ctx, id)
        .await
        .expect("the contract");
    tx.commit().await.expect("to commit");
    found
}

// ---------------------------------------------------------------------------
// Renewing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_contract_that_is_due_is_billed_and_its_clock_moves_on() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;
    let before = read(&shop, seeded.id).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let owed = subscription::due(&mut tx, &ctx, Utc::now(), Paging::first(10))
        .await
        .expect("what is due");
    tx.commit().await.expect("to commit");
    assert_eq!(owed.len(), 1, "the overdue contract was not offered");

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run");

    assert!(renewed.order_id.is_some(), "no order came out of it");
    assert!(!renewed.declined);
    assert_eq!(billed_cycles(&shop, seeded.id).await, vec![1]);

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.cycle, 1);
    assert_eq!(after.status, "active");
    assert!(
        after.next_billing_at > before.next_billing_at,
        "the contract is still owed the period it just billed"
    );
    assert_eq!(after.current_period_start, before.current_period_end);
    assert!(shop.host.emitted("subscription.renewed"));

    shop.close().await;
}

/// Two schedulers firing at the same moment, on two connections. Not one after
/// the other: what is being asserted is that the key and the unique cycle hold
/// against a genuine race.
#[tokio::test]
async fn two_renewals_at_once_bill_the_period_once() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let one = shop.ctx();
    let two = shop.ctx();

    let (first, second) = tokio::join!(
        renewals.renew(&shop.pool, &one, seeded.id),
        renewals.renew(&shop.pool, &two, seeded.id),
    );

    assert!(
        first.is_ok() || second.is_ok(),
        "neither renewal got anywhere: {first:?} / {second:?}"
    );
    assert_eq!(orders(&shop).await, 1, "the period was billed twice");
    assert_eq!(billed_cycles(&shop, seeded.id).await, vec![1]);
    assert_eq!(read(&shop, seeded.id).await.cycle, 1);

    shop.close().await;
}

#[tokio::test]
async fn a_renewal_charges_what_the_price_is_now_rather_than_what_was_agreed() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    pricing::update_price(
        &mut tx,
        &ctx,
        seeded.price_id,
        pricing::PriceUpdate {
            amount: Some(Money::new(dec!(12), try_())),
            ..pricing::PriceUpdate::default()
        },
    )
    .await
    .expect("the price to move");
    tx.commit().await.expect("to commit");

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run");
    let order_id = renewed.order_id.expect("an order");

    let mut tx = shop.begin().await;
    let charged: Decimal = sqlx::query_scalar(
        "select unit_price from order_line_item where scope = $1 and order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the line");
    tx.commit().await.expect("to commit");

    assert_eq!(charged, dec!(12), "the contract's copy was billed instead");

    let after = read(&shop, seeded.id).await;
    assert_eq!(
        after.line_version, 2,
        "a price that moved was edited in place rather than versioned"
    );

    let mut tx = shop.begin().await;
    let events = subscription::events(&mut tx, &shop.ctx(), seeded.id, Paging::first(20))
        .await
        .expect("the log");
    tx.commit().await.expect("to commit");
    assert!(
        events
            .items
            .iter()
            .any(|event| event.kind == "price_changed"),
        "nothing in the log says the price moved"
    );

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Dunning
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_declined_charge_leaves_the_contract_past_due_with_a_retry_queued() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let renewals = Renewals::new(Arc::new(Refusing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run and be refused");

    assert!(renewed.declined);
    assert!(!renewed.cancelled);
    assert!(renewed.order_id.is_none(), "a refused charge left an order");
    assert_eq!(orders(&shop).await, 0, "the unpaid order was not unwound");

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.status, "past_due");
    assert_eq!(after.dunning_attempts, 1);
    assert!(
        billed_cycles(&shop, seeded.id).await.is_empty(),
        "a period nobody paid for was recorded as billed"
    );

    let queued = shop.host.queued_for("subscription.dunning");
    assert_eq!(queued.len(), 1, "no retry was queued");
    let run_after = queued[0].expect("a retry with a time on it");
    assert!(
        run_after > Utc::now() + Duration::hours(1),
        "the retry asked to run immediately, which is the same card in the same minute"
    );

    shop.close().await;
}

/// #163: `create_order`'s own undo used to be a shorter, separately typed
/// copy of checkout's. `reserve_stock`'s own compensate already gives its
/// hold back by reservation id regardless of what `create_order`'s does, so
/// this does not catch a hard failure on today's plain renewal — nothing
/// currently rebinds the hold onto the order line the way checkout does. It
/// locks in the outcome a renewal's rewind has to keep once that changes: a
/// clean revert, nothing dead-lettered, the stock back where it was.
#[tokio::test]
async fn a_renewal_declined_after_the_order_is_written_unwinds_cleanly() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let item = inventory::inventory_items_for_variant(&mut tx, &ctx, seeded.variant_id)
        .await
        .expect("an inventory item")
        .first()
        .expect("an inventory item")
        .inventory_item_id;
    let before = inventory::level(&mut tx, &ctx, item, seeded.location_id)
        .await
        .expect("a level");
    tx.commit().await.expect("to commit");

    let renewals = Renewals::new(Arc::new(Refusing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run and be refused");

    assert!(renewed.declined);
    assert_eq!(
        renewed.run.state,
        State::Reverted,
        "the compensation itself failed rather than the charge: {:?}",
        renewed.run.failure
    );

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let dead_letters: i64 = sqlx::query_scalar(
        "select count(*) from workflow_dead_letter where scope = $1 and run_id = $2",
    )
    .bind(shop.here.0)
    .bind(renewed.run.id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("to count dead letters");
    assert_eq!(dead_letters, 0, "a step's undo could not run");

    let after = inventory::level(&mut tx, &ctx, item, seeded.location_id)
        .await
        .expect("a level");
    tx.commit().await.expect("to commit");

    assert_eq!(
        after.reserved_quantity, before.reserved_quantity,
        "the renewal's reservation was never given back"
    );
    assert_eq!(orders(&shop).await, 0, "the unpaid order was not unwound");

    shop.close().await;
}

/// #165: a renewal's hold used to name no line at all, so an operator looking
/// at held stock on a subscription order had no way back to it — the same
/// join checkout's order lines already answer.
#[tokio::test]
async fn a_renewal_reservation_names_its_order_line() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run");
    let order_id = renewed.order_id.expect("a paid renewal writes an order");

    let mut tx = shop.begin().await;
    let held: i64 = sqlx::query_scalar(
        "select count(*) from reservation_item ri
         join order_line_item l on l.scope = ri.scope and l.id = ri.order_line_item_id
         where ri.scope = $1 and l.order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("to count reservations joined to the order's own line");
    tx.commit().await.expect("to commit");

    assert_eq!(
        held, 1,
        "the renewal's hold names no line the order can be joined to"
    );

    shop.close().await;
}

/// #165: `release_lines` releases by line id, and a renewal's reservation
/// used to name none — nothing but `reserve_stock`'s own compensate could
/// ever give a renewal's stock back. This is the same release a cancelled
/// checkout order already gets.
#[tokio::test]
async fn release_lines_gives_a_renewal_order_its_stock_back() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run");
    let order_id = renewed.order_id.expect("a paid renewal writes an order");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let item = inventory::inventory_items_for_variant(&mut tx, &ctx, seeded.variant_id)
        .await
        .expect("an inventory item")
        .first()
        .expect("an inventory item")
        .inventory_item_id;
    let before = inventory::level(&mut tx, &ctx, item, seeded.location_id)
        .await
        .expect("a level");

    let line_ids: Vec<Uuid> =
        sqlx::query_scalar("select id from order_line_item where scope = $1 and order_id = $2")
            .bind(shop.here.0)
            .bind(order_id.as_uuid())
            .fetch_all(&mut *tx)
            .await
            .expect("the order's lines");
    let lines: Vec<LineItemId> = line_ids.into_iter().map(LineItemId::from_uuid).collect();

    let released = inventory::release_lines(&mut tx, &ctx, &lines)
        .await
        .expect("release_lines to reach a renewal order's line");
    assert_eq!(
        released, 1,
        "the renewal's own reservation was not released"
    );

    let after = inventory::level(&mut tx, &ctx, item, seeded.location_id)
        .await
        .expect("a level");
    tx.commit().await.expect("to commit");

    assert_eq!(
        after.reserved_quantity,
        before.reserved_quantity - 1,
        "release_lines did not give the renewal's held unit back"
    );

    shop.close().await;
}

/// The status and the job are one transaction. A queue that refuses takes the
/// `past_due` with it rather than leaving a contract nothing will come back to.
#[tokio::test]
async fn a_queue_that_refuses_takes_the_status_change_with_it() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let host = NoJobs;
    let ctx = shop.ctx_as(Actor::System, &host as &dyn Host);
    let renewals = Renewals::new(Arc::new(Refusing), seeded.location_id);

    let refused = renewals.renew(&shop.pool, &ctx, seeded.id).await;
    assert!(refused.is_err(), "a broken queue was reported as success");

    let after = read(&shop, seeded.id).await;
    assert_eq!(
        after.status, "active",
        "the contract is past due with nothing queued to come back to it"
    );
    assert_eq!(after.dunning_attempts, 0);

    shop.close().await;
}

#[tokio::test]
async fn three_declines_over_a_fortnight_end_in_exactly_one_cancellation() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;
    let renewals = Renewals::new(Arc::new(Refusing), seeded.location_id);

    let mut cancellations = 0;
    for _ in 0..3 {
        let renewed = renewals
            .renew(&shop.pool, &shop.ctx(), seeded.id)
            .await
            .expect("each attempt to run");
        assert!(renewed.declined);
        if renewed.cancelled {
            cancellations += 1;
        }
    }

    assert_eq!(
        cancellations, 1,
        "the contract was cancelled {cancellations} times"
    );

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.status, "cancelled");
    assert!(after.ended_at.is_some());
    assert_eq!(after.dunning_attempts, 3);
    assert_eq!(
        shop.host.payloads_of("subscription.cancelled").len(),
        1,
        "more than one cancellation was announced"
    );

    // A fourth poll finds a contract that no longer renews and says so rather
    // than cancelling it again.
    let again = renewals.renew(&shop.pool, &shop.ctx(), seeded.id).await;
    assert!(again.is_ok());
    assert_eq!(
        shop.host.payloads_of("subscription.cancelled").len(),
        1,
        "polling a cancelled contract cancelled it again"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_failed_charge_leaves_the_period_where_it_was_and_the_retry_continues_it() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;
    let before = read(&shop, seeded.id).await;

    let refusing = Renewals::new(Arc::new(Refusing), seeded.location_id);
    refusing
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the first attempt to run");

    let stalled = read(&shop, seeded.id).await;
    assert_eq!(
        stalled.next_billing_at, before.next_billing_at,
        "a failure moved the clock, so the cycle it failed on will never be billed"
    );
    assert_eq!(stalled.cycle, before.cycle);

    let standing = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = standing
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the retry to run");

    assert_eq!(renewed.cycle, 1, "the retry billed another period");
    assert_eq!(billed_cycles(&shop, seeded.id).await, vec![1]);

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.status, "active");
    assert_eq!(after.dunning_attempts, 0);

    shop.close().await;
}

/// Off-session, a second factor is a decline: there is nobody in a browser to
/// send anywhere.
#[tokio::test]
async fn a_provider_asking_for_the_shopper_is_a_decline_rather_than_a_redirect() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let renewals = Renewals::new(Arc::new(Insisting), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal to run");

    assert!(renewed.declined, "a hold nobody can complete was accepted");
    assert_eq!(read(&shop, seeded.id).await.status, "past_due");

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Who is asking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_host_that_denies_the_system_stops_renewals_loudly() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let host = NoSystem;
    let ctx = shop.ctx_as(Actor::System, &host as &dyn Host);
    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);

    let refused = renewals
        .renew(&shop.pool, &ctx, seeded.id)
        .await
        .expect_err("a denied renewal to be an error rather than a shrug");

    assert!(refused.is_denied());
    assert_eq!(orders(&shop).await, 0);
    assert_eq!(read(&shop, seeded.id).await.cycle, 0);

    shop.close().await;
}

// ---------------------------------------------------------------------------
// The contract itself
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_contract_cancelled_at_period_end_stops_renewing_without_ending_now() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let stopped = subscription::cancel(&mut tx, &ctx, seeded.id, true, Some("too much coffee"))
        .await
        .expect("to stop it");
    tx.commit().await.expect("to commit");

    assert!(stopped.cancel_at_period_end);
    assert_eq!(stopped.status, "active");
    assert!(stopped.ended_at.is_none());

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the poll to run");

    assert_eq!(orders(&shop).await, 0, "a contract asked to stop renewed");

    shop.close().await;
}

#[tokio::test]
async fn a_contract_that_has_had_every_cycle_it_was_sold_expires() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), Some(1)).await;

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the one cycle it was sold");

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.status, "expired");
    assert!(after.ended_at.is_some());

    let again = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the poll to run");
    assert!(again.order_id.is_none());
    assert_eq!(billed_cycles(&shop, seeded.id).await, vec![1]);

    shop.close().await;
}

#[tokio::test]
async fn a_contract_belongs_to_the_customer_and_the_plan_it_was_opened_on() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let mine = subscription::list(&mut tx, &ctx, Some(seeded.customer_id), Paging::first(10))
        .await
        .expect("my contracts");
    assert_eq!(mine.len(), 1);

    let lines = subscription::lines(&mut tx, &ctx, seeded.id)
        .await
        .expect("its lines");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].variant_id, seeded.variant_id);

    let found = subscription::get(&mut tx, &ctx, seeded.id)
        .await
        .expect("the contract");
    assert_eq!(found.selling_plan_id, seeded.plan_id);
    assert_eq!(found.account_holder_id, Some(seeded.holder_id));

    tx.commit().await.expect("to commit");
    shop.close().await;
}

// ---------------------------------------------------------------------------
// Pause, resume, skip and swap
// ---------------------------------------------------------------------------

fn a_month_plan() -> NewPlan {
    NewPlan {
        name: "Every month".into(),
        billing_interval_unit: "month".into(),
        billing_interval_count: 1,
        ..NewPlan::default()
    }
}

#[tokio::test]
async fn a_paused_contract_is_not_due_and_resuming_does_not_bill_for_what_it_missed() {
    let shop = Shop::open().await;
    // Backdated sixty days, the same as `a_contract`: the first period ended
    // about a month ago, so `next_billing_at` is already in the past.
    let seeded = a_contract(&shop, dec!(10), None).await;
    let overdue = read(&shop, seeded.id).await;
    assert!(overdue.next_billing_at < Utc::now());

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    subscription::pause(&mut tx, &ctx, seeded.id, None)
        .await
        .expect("to pause");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let owed = subscription::due(&mut tx, &ctx, Utc::now(), Paging::first(10))
        .await
        .expect("what is due");
    assert!(
        !owed.items.iter().any(|s| s.id == seeded.id),
        "a paused contract must not come back from due()"
    );
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let resumed = subscription::resume(&mut tx, &ctx, seeded.id)
        .await
        .expect("to resume");
    tx.commit().await.expect("to commit");

    assert_eq!(resumed.status, "active");
    // The calendar shifted forward to the moment it resumed rather than
    // catching up on the month it missed while paused.
    assert!(resumed.next_billing_at >= Utc::now() - Duration::seconds(5));
    assert!(resumed.next_billing_at > overdue.next_billing_at);

    shop.close().await;
}

#[tokio::test]
async fn skipping_passes_exactly_one_period_and_writes_an_event() {
    let shop = Shop::open().await;
    let seeded = a_contract(&shop, dec!(10), None).await;
    let before = read(&shop, seeded.id).await;
    let orders_before = orders(&shop).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let after = subscription::skip_next(&mut tx, &ctx, seeded.id)
        .await
        .expect("to skip");
    tx.commit().await.expect("to commit");

    assert_eq!(after.cycle, before.cycle + 1);
    assert_eq!(orders(&shop).await, orders_before, "a skip bills nothing");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let events = subscription::events(&mut tx, &ctx, seeded.id, Paging::first(20))
        .await
        .expect("its events");
    tx.commit().await.expect("to commit");
    assert!(events.items.iter().any(|e| e.kind == "skipped"));

    shop.close().await;
}

#[tokio::test]
async fn swapping_writes_a_new_line_version_and_the_next_renewal_bills_it() {
    let shop = Shop::open().await;
    let mut plan = a_month_plan();
    plan.name = "Swap plan".into();
    let seeded = a_contract_with(&shop, dec!(10), plan).await;
    let bigger = a_second_variant(&shop, seeded.plan_id, dec!(20)).await;

    // A third of the period already used, so the proration split is nowhere
    // near either edge and the assertions below are not at the mercy of
    // rounding a near-whole period to the whole cent.
    let mut tx = shop.begin().await;
    sqlx::query(
        "update subscription
         set current_period_start = $2, current_period_end = $3, next_billing_at = $3
         where scope = $1 and id = $4",
    )
    .bind(shop.here.0)
    .bind(Utc::now() - Duration::days(20))
    .bind(Utc::now() + Duration::days(10))
    .bind(seeded.id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to set a mid-period clock");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let before = subscription::get(&mut tx, &ctx, seeded.id)
        .await
        .expect("the contract");
    let swapped = subscription::swap(
        &mut tx,
        &ctx,
        seeded.id,
        vec![NewLine {
            variant_id: bigger,
            title: Some("The bigger one".into()),
            quantity: 1,
            unit_price: Money::new(dec!(20), try_()),
        }],
    )
    .await
    .expect("to swap");
    tx.commit().await.expect("to commit");

    assert_eq!(swapped.line_version, before.line_version + 1);
    // Upgrading mid-period costs more for the days left than the old lines
    // did: nothing is charged off-session on the spot, so it waits here.
    assert!(swapped.pending_adjustment > Decimal::ZERO);
    assert!(swapped.pending_adjustment < dec!(10));

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let held = subscription::lines(&mut tx, &ctx, seeded.id)
        .await
        .expect("its lines");
    tx.commit().await.expect("to commit");
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].variant_id, bigger);

    // Force the contract due, then let the next renewal collect the
    // adjustment along with the bigger variant's own price.
    let mut tx = shop.begin().await;
    sqlx::query("update subscription set next_billing_at = $2 where scope = $1 and id = $3")
        .bind(shop.here.0)
        .bind(Utc::now() - Duration::minutes(1))
        .bind(seeded.id.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("to force it due");
    tx.commit().await.expect("to commit");

    let renewals = Renewals::new(Arc::new(Standing), seeded.location_id);
    let renewed = renewals
        .renew(&shop.pool, &shop.ctx(), seeded.id)
        .await
        .expect("the renewal");
    assert!(!renewed.declined);

    let after = read(&shop, seeded.id).await;
    assert_eq!(
        after.pending_adjustment,
        Decimal::ZERO,
        "the balance is collected once and then cleared"
    );

    shop.close().await;
}

#[tokio::test]
async fn downgrading_mid_period_is_credited_on_the_spot() {
    let shop = Shop::open().await;
    let mut plan = a_month_plan();
    plan.name = "Downgrade plan".into();
    let seeded = a_contract_with(&shop, dec!(20), plan).await;
    let cheaper = a_second_variant(&shop, seeded.plan_id, dec!(5)).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    subscription::swap(
        &mut tx,
        &ctx,
        seeded.id,
        vec![NewLine {
            variant_id: cheaper,
            title: Some("The cheaper one".into()),
            quantity: 1,
            unit_price: Money::new(dec!(5), try_()),
        }],
    )
    .await
    .expect("to swap down");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let balance = credit::store_credit(&mut tx, &ctx, seeded.customer_id, try_())
        .await
        .expect("a credit balance");
    tx.commit().await.expect("to commit");

    assert!(balance.balance > Decimal::ZERO);

    shop.close().await;
}

#[tokio::test]
async fn a_minimum_term_refuses_an_immediate_cancel_but_allows_one_at_period_end() {
    let shop = Shop::open().await;
    let mut plan = a_month_plan();
    plan.name = "Committed plan".into();
    plan.min_cycles = Some(3);
    let seeded = a_contract_with(&shop, dec!(10), plan).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let refused = subscription::cancel(&mut tx, &ctx, seeded.id, false, None).await;
    assert!(refused.is_err(), "a minimum term is not done yet");
    tx.rollback().await.expect("to roll back");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let allowed = subscription::cancel(&mut tx, &ctx, seeded.id, true, None)
        .await
        .expect("to stop at period end even short of the minimum");
    tx.commit().await.expect("to commit");
    assert!(allowed.cancel_at_period_end);

    shop.close().await;
}

// ---------------------------------------------------------------------------
// Prepaid terms
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_prepaid_term_ships_a_delivery_without_charging_anything() {
    let shop = Shop::open().await;
    let plan = NewPlan {
        name: "Six months, monthly boxes".into(),
        billing_interval_unit: "month".into(),
        billing_interval_count: 6,
        delivery_interval_unit: Some("month".into()),
        delivery_interval_count: Some(1),
        prepaid_cycles: Some(6),
        ..NewPlan::default()
    };
    let seeded = a_contract_with(&shop, dec!(60), plan).await;

    // The bundled first delivery went out with the order the contract opened
    // under; back-date the contract so the second one is already due.
    let mut tx = shop.begin().await;
    sqlx::query(
        "update subscription set next_delivery_at = $2, current_period_start = $2
         where scope = $1 and id = $3",
    )
    .bind(shop.here.0)
    .bind(Utc::now() - Duration::days(1))
    .bind(seeded.id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to force a delivery due");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let owed = subscription::due_deliveries(&mut tx, &ctx, Utc::now(), Paging::first(10))
        .await
        .expect("what is owed a delivery");
    assert!(owed.items.iter().any(|s| s.id == seeded.id));
    tx.commit().await.expect("to commit");

    let orders_before = orders(&shop).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let order_id = subscription::deliver(&mut tx, &ctx, seeded.id, seeded.location_id)
        .await
        .expect("to ship the delivery");
    tx.commit().await.expect("to commit");

    assert_eq!(orders(&shop).await, orders_before + 1);

    let mut tx = shop.begin().await;
    let charged: Option<Uuid> = sqlx::query_scalar(
        r#"select payment_collection_id from "order" where scope = $1 and id = $2"#,
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the order");
    tx.commit().await.expect("to commit");
    assert!(charged.is_none(), "a prepaid delivery takes no money");

    let after = read(&shop, seeded.id).await;
    assert_eq!(after.delivery_cycle, 1);

    shop.close().await;
}
