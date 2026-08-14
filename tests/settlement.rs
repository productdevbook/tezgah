//! What must happen because money arrived, and only once however often it is
//! told to arrive.

mod common;

use async_trait::async_trait;
use common::Shop;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::customer;
use tezgah::digital::{self, NewContent};
use tezgah::id::{OrderId, PaymentId, PaymentSessionId, SubscriptionId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, Authorized, CancelRequest,
    CaptureRequest, CaptureResult, NewCollection, NewSession, PaymentProvider, RefundRequest,
    RefundResult, SessionRequest, SessionResponse, SessionStatus, WebhookEvent,
};
use tezgah::ports::{Ctx, Tx};
use tezgah::settlement;
use tezgah::subscription::{self, NewLine, NewPlan, NewPlanGroup, NewSubscription};

const PROVIDER: &str = "fake";

#[derive(Debug, Default)]
struct FakeProvider;

#[async_trait]
impl PaymentProvider for FakeProvider {
    fn code(&self) -> &'static str {
        PROVIDER
    }

    async fn create_session(&self, req: SessionRequest) -> tezgah::Result<SessionResponse> {
        Ok(SessionResponse {
            data: json!({ "intent": req.session_id.to_string() }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> tezgah::Result<Authorization> {
        Ok(Authorization {
            status: AuthorizationStatus::Authorized,
            amount: Some(req.amount),
            data: json!({}),
            redirect: None,
            message: None,
            installment: None,
        })
    }

    async fn capture(&self, req: CaptureRequest) -> tezgah::Result<CaptureResult> {
        Ok(CaptureResult {
            amount: req.amount,
            data: json!({}),
        })
    }

    async fn refund(&self, req: RefundRequest) -> tezgah::Result<RefundResult> {
        Ok(RefundResult {
            amount: req.amount,
            data: json!({}),
        })
    }

    async fn cancel(&self, _: CancelRequest) -> tezgah::Result<()> {
        Ok(())
    }

    fn parse_webhook(&self, _: &[(String, String)], _: &[u8]) -> tezgah::Result<WebhookEvent> {
        Err(tezgah::Error::invalid("not exercised here"))
    }
}

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: rust_decimal::Decimal) -> Money {
    Money::new(amount, lira())
}

async fn a_variant(tx: &mut Tx<'_>, ctx: &Ctx<'_>, handle: &str) -> VariantId {
    let product = catalogue::create_product(
        tx,
        ctx,
        NewProduct {
            handle: handle.into(),
            title: format!("A {handle}"),
            ..NewProduct::default()
        },
    )
    .await
    .expect("a product");

    catalogue::create_variant(
        tx,
        ctx,
        product.id,
        NewVariant {
            title: "One size".into(),
            sku: Some(format!("{handle}-1")),
            ..NewVariant::default()
        },
    )
    .await
    .expect("a variant")
    .id
}

/// A registered provider, a collection for `total`, and an open session on it.
async fn open_session(shop: &Shop, total: Money) -> PaymentSessionId {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    payment::register_provider(&mut tx, &ctx, PROVIDER)
        .await
        .expect("a provider");

    let collection = payment::create_collection(
        &mut tx,
        &ctx,
        NewCollection {
            amount: total,
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a collection");

    let session = payment::create_session(
        &mut tx,
        &ctx,
        NewSession {
            collection_id: collection.id,
            provider_code: PROVIDER.to_owned(),
            amount: total,
            context: None,
            installment_count: None,
        },
    )
    .await
    .expect("a session");

    tx.commit().await.expect("to commit");
    session.id
}

async fn authorize(shop: &Shop, session: PaymentSessionId) -> Authorized {
    let ctx = shop.ctx();

    let asked = {
        let mut tx = shop.begin().await;
        let row = payment::session(&mut tx, &ctx, session)
            .await
            .expect("the session");
        tx.commit().await.expect("to commit");
        row.money().expect("an amount")
    };

    let answer = FakeProvider
        .authorize(AuthorizeRequest {
            session_id: session,
            amount: asked,
            data: json!({}),
            context: json!({}),
            installment_count: None,
        })
        .await
        .expect("the provider to answer");

    let mut tx = shop.begin().await;
    let outcome = payment::authorize(&mut tx, &ctx, session, answer)
        .await
        .expect("to record the authorisation");
    tx.commit().await.expect("to commit");
    outcome
}

/// A plain order for `total`, its own collection carrying its own session,
/// authorised and ready to be captured. `metadata` is the order's, so a test
/// can attach a `subscription` id the way checkout would.
async fn an_order(
    shop: &Shop,
    total: Money,
    lines: Vec<NewOrderLine>,
    metadata: Option<serde_json::Value>,
) -> (OrderId, PaymentId) {
    let session = open_session(shop, total).await;
    let outcome = authorize(shop, session).await;
    let paid = outcome.payment().expect("a payment").id;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let collection = payment::session(&mut tx, &ctx, session)
        .await
        .expect("the session")
        .payment_collection_id;

    let placed = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            payment_collection_id: Some(collection),
            lines,
            metadata,
            ..NewOrder::of(total.currency)
        },
    )
    .await
    .expect("an order");

    tx.commit().await.expect("to commit");
    (placed.id, paid)
}

async fn giftcards_of(shop: &Shop, order_id: OrderId) -> i64 {
    let mut tx = shop.begin().await;
    let count = sqlx::query_scalar(
        "select count(*) from gift_card where scope = $1 and issued_order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the gift cards this order printed");
    tx.rollback().await.expect("to roll back");
    count
}

async fn entitlements_of(shop: &Shop, order_id: OrderId) -> i64 {
    let mut tx = shop.begin().await;
    let count = sqlx::query_scalar(
        "select count(*) from order_entitlement where scope = $1 and order_id = $2 and revoked_at is null",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the live entitlements this order granted");
    tx.rollback().await.expect("to roll back");
    count
}

async fn periods_of(shop: &Shop, subscription_id: SubscriptionId) -> Vec<(i32, String)> {
    let mut tx = shop.begin().await;
    let rows = sqlx::query_as(
        "select cycle, kind from subscription_order
         where scope = $1 and subscription_id = $2
         order by cycle",
    )
    .bind(shop.here.0)
    .bind(subscription_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the periods billed so far");
    tx.rollback().await.expect("to roll back");
    rows
}

struct Seed {
    giftcard_variant: VariantId,
    digital_variant: VariantId,
    subscription_id: SubscriptionId,
    subscription_variant: VariantId,
}

/// A shop selling a gift card, a digital file, and one subscription plan —
/// enough to exercise every path `settlement::capture` fans out to.
async fn a_seeded_shop() -> (Shop, Seed) {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let giftcard_variant = a_variant(&mut tx, &ctx, "card").await;

    let digital_variant = a_variant(&mut tx, &ctx, "ebook").await;
    digital::put_content(
        &mut tx,
        &ctx,
        digital_variant,
        NewContent {
            content_key: "books/ebook.epub".into(),
            name: "The book".into(),
            max_downloads: Some(3),
            valid_days: None,
            auto_grant: Some(true),
            rank: None,
            metadata: None,
        },
    )
    .await
    .expect("the file to go on the variant");

    let subscription_variant = a_variant(&mut tx, &ctx, "monthly").await;
    let customer_id = customer::create(&mut tx, &ctx, customer::NewCustomer::guest())
        .await
        .expect("a customer")
        .id;

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
            ..NewPlan::default()
        },
    )
    .await
    .expect("a plan");

    subscription::attach_variant(&mut tx, &ctx, plan.id, subscription_variant)
        .await
        .expect("the variant to be sold on it");

    let contract = subscription::create(
        &mut tx,
        &ctx,
        NewSubscription {
            customer_id,
            selling_plan_id: plan.id,
            currency: lira(),
            region_id: None,
            sales_channel_id: None,
            account_holder_id: None,
            payment_method_reference: None,
            mandate_reference: None,
            mandate_accepted_at: None,
            shipping_address_id: None,
            billing_address_id: None,
            starts_at: None,
            lines: vec![NewLine {
                variant_id: subscription_variant,
                title: Some("A thing, monthly".into()),
                quantity: 1,
                unit_price: money(dec!(50.00)),
            }],
        },
    )
    .await
    .expect("a contract");

    tx.commit().await.expect("to commit the seed");

    (
        shop,
        Seed {
            giftcard_variant,
            digital_variant,
            subscription_id: contract.id,
            subscription_variant,
        },
    )
}

/// Capturing prints the gift card, grants the entitlement, and starts the
/// subscription's first period — all in the transaction that took the money.
#[tokio::test]
async fn capturing_does_everything_money_arriving_obligates() {
    let (shop, seed) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(150.00)),
        vec![
            NewOrderLine {
                variant_id: Some(seed.giftcard_variant),
                is_giftcard: true,
                requires_shipping: false,
                ..NewOrderLine::of("A card", 1, money(dec!(50.00)))
            },
            NewOrderLine {
                variant_id: Some(seed.digital_variant),
                requires_shipping: false,
                ..NewOrderLine::of("The book", 1, money(dec!(50.00)))
            },
            NewOrderLine {
                variant_id: Some(seed.subscription_variant),
                requires_shipping: false,
                ..NewOrderLine::of("A thing, monthly", 1, money(dec!(50.00)))
            },
        ],
        Some(json!({ "subscription": seed.subscription_id })),
    )
    .await;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(150.00)), None)
        .await
        .expect("the capture to settle everything");
    tx.commit().await.expect("to commit");

    assert_eq!(giftcards_of(&shop, order_id).await, 1);
    assert_eq!(entitlements_of(&shop, order_id).await, 1);
    assert_eq!(
        periods_of(&shop, seed.subscription_id).await,
        vec![(0, "initial".to_string())]
    );

    shop.close().await;
}

/// A provider redelivers its webhook; a second `settlement::capture` on the
/// same payment settles nothing a second time.
#[tokio::test]
async fn a_second_capture_is_a_no_op() {
    let (shop, seed) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(100.00)),
        vec![
            NewOrderLine {
                variant_id: Some(seed.giftcard_variant),
                is_giftcard: true,
                requires_shipping: false,
                ..NewOrderLine::of("A card", 1, money(dec!(50.00)))
            },
            NewOrderLine {
                variant_id: Some(seed.subscription_variant),
                requires_shipping: false,
                ..NewOrderLine::of("A thing, monthly", 1, money(dec!(50.00)))
            },
        ],
        Some(json!({ "subscription": seed.subscription_id })),
    )
    .await;

    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(60.00)), None)
        .await
        .expect("the first capture");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(40.00)), None)
        .await
        .expect("the second, completing, capture");
    tx.commit().await.expect("to commit");

    assert_eq!(giftcards_of(&shop, order_id).await, 1, "one card, not two");
    assert_eq!(
        periods_of(&shop, seed.subscription_id).await,
        vec![(0, "initial".to_string())],
        "one period, not two"
    );

    shop.close().await;
}

/// Authorising, not capturing, obligates nothing yet.
#[tokio::test]
async fn authorising_alone_settles_nothing() {
    let (shop, seed) = a_seeded_shop().await;

    let (order_id, _paid) = an_order(
        &shop,
        money(dec!(50.00)),
        vec![NewOrderLine {
            variant_id: Some(seed.giftcard_variant),
            is_giftcard: true,
            requires_shipping: false,
            ..NewOrderLine::of("A card", 1, money(dec!(50.00)))
        }],
        Some(json!({ "subscription": seed.subscription_id })),
    )
    .await;

    assert_eq!(giftcards_of(&shop, order_id).await, 0);
    assert_eq!(entitlements_of(&shop, order_id).await, 0);
    assert!(periods_of(&shop, seed.subscription_id).await.is_empty());

    shop.close().await;
}

/// Refunding revokes the digital entitlement and leaves the printed gift
/// card untouched: capture has no compensation in this crate, and a refund
/// is not one either.
#[tokio::test]
async fn refunding_revokes_the_entitlement_and_leaves_the_card_alone() {
    let (shop, seed) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(100.00)),
        vec![
            NewOrderLine {
                variant_id: Some(seed.giftcard_variant),
                is_giftcard: true,
                requires_shipping: false,
                ..NewOrderLine::of("A card", 1, money(dec!(50.00)))
            },
            NewOrderLine {
                variant_id: Some(seed.digital_variant),
                requires_shipping: false,
                ..NewOrderLine::of("The book", 1, money(dec!(50.00)))
            },
        ],
        None,
    )
    .await;

    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(100.00)), None)
        .await
        .expect("the capture");
    tx.commit().await.expect("to commit");

    assert_eq!(giftcards_of(&shop, order_id).await, 1);
    assert_eq!(entitlements_of(&shop, order_id).await, 1);

    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(100.00)), None, None)
        .await
        .expect("the refund");
    tx.commit().await.expect("to commit");

    assert_eq!(
        entitlements_of(&shop, order_id).await,
        0,
        "the file must not still be downloadable"
    );
    assert_eq!(
        giftcards_of(&shop, order_id).await,
        1,
        "a captured-then-refunded card is left alone"
    );

    shop.close().await;
}
