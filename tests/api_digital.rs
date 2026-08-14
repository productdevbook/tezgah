//! Digital products from the route in, because five features have shipped
//! green and unreachable and this is the half a domain test cannot see.

mod common;

use common::{OnlyMine, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::api::admin_order::{
    self, CapturePayment, CreateOrder, MoneyIn, NewLineIn, RefundPayment,
};
use tezgah::api::digital as route;
use tezgah::catalogue::{self, NewProduct, NewVariant};
use tezgah::customer::{self, NewCustomer};
use tezgah::id::{CustomerId, OrderId, PaymentId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, NewCollection, NewSession, SessionResponse,
    SessionStatus,
};
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};
use uuid::Uuid;

const PROVIDER: &str = "fake";

fn money(amount: Decimal) -> Money {
    Money::new(amount, Currency::parse("TRY").expect("a currency code"))
}

fn try_(amount: Decimal) -> MoneyIn {
    MoneyIn {
        amount,
        currency: "TRY".to_owned(),
    }
}

async fn seed_currency(tx: &mut Tx<'_>, scope: Scope) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

/// A variant with one file on it, put there through the admin route.
async fn a_sellable_file(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> VariantId {
    let product = catalogue::create_product(
        tx,
        ctx,
        NewProduct {
            handle: "novel".into(),
            title: "A novel".into(),
            ..NewProduct::default()
        },
    )
    .await
    .expect("a product");

    let variant = catalogue::create_variant(
        tx,
        ctx,
        product.id,
        NewVariant {
            title: "epub".into(),
            sku: Some("novel-1".into()),
            ..NewVariant::default()
        },
    )
    .await
    .expect("a variant")
    .id;

    let put = route::put_content(
        tx,
        ctx,
        variant,
        route::PutContent {
            content_key: "books/novel.epub".into(),
            name: "A novel".into(),
            max_downloads: Some(2),
            ..route::PutContent::default()
        },
    )
    .await
    .expect("the file to go on");
    assert_eq!(put.max_downloads, Some(2));

    let listed = route::list_content(tx, ctx, variant)
        .await
        .expect("what the variant carries");
    assert_eq!(listed.len(), 1);

    variant
}

fn an_order(variant: VariantId, customer: Option<CustomerId>, amount: Decimal) -> CreateOrder {
    CreateOrder {
        currency: "TRY".to_owned(),
        email: Some("shopper@example.com".into()),
        customer_id: customer,
        region_id: None,
        sales_channel_id: None,
        locale: None,
        lines: vec![NewLineIn {
            variant_id: Some(variant),
            product_id: None,
            title: "A novel".into(),
            quantity: 1,
            unit_price: try_(amount),
            requires_shipping: false,
            is_tax_inclusive: false,
            discount: Decimal::ZERO,
            tax_rate: Decimal::ZERO,
            withdrawal_exclusion: None,
            is_giftcard: false,
        }],
        shipping: Vec::new(),
        metadata: None,
    }
}

/// A payment with `total` held against it and the order paying through it.
async fn a_held_payment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    scope: Scope,
    order_id: OrderId,
    total: Money,
) -> PaymentId {
    payment::register_provider(tx, ctx, PROVIDER)
        .await
        .expect("a provider");

    let collection = payment::create_collection(
        tx,
        ctx,
        NewCollection {
            amount: total,
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a collection");

    let session = payment::create_session(
        tx,
        ctx,
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

    payment::record_session(
        tx,
        ctx,
        session.id,
        SessionResponse {
            data: json!({}),
            status: SessionStatus::Pending,
        },
    )
    .await
    .expect("the session to be written back");

    let held = payment::authorize(
        tx,
        ctx,
        session.id,
        Authorization {
            status: AuthorizationStatus::Authorized,
            amount: Some(total),
            data: json!({}),
            redirect: None,
            message: None,
            installment: None,
        },
    )
    .await
    .expect("to record the authorisation")
    .payment()
    .expect("a payment")
    .id;

    sqlx::query(r#"update "order" set payment_collection_id = $3 where scope = $1 and id = $2"#)
        .bind(scope.0)
        .bind(order_id.as_uuid())
        .bind(collection.id.as_uuid())
        .execute(&mut **tx)
        .await
        .expect("the order to be paying through it");

    held
}

/// The route that takes the money is the route that hands the file over, and
/// the route that gives it back is the one that takes the file away.
#[tokio::test]
async fn capturing_grants_and_refunding_takes_it_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let variant = a_sellable_file(&mut tx, &ctx).await;
    let placed = admin_order::create_order(&mut tx, &ctx, an_order(variant, None, dec!(100.00)))
        .await
        .expect("an order");
    let held = a_held_payment(&mut tx, &ctx, shop.here, placed.id, money(dec!(100.00))).await;

    assert!(
        route::list_order_entitlements(&mut tx, &ctx, placed.id)
            .await
            .expect("nothing yet")
            .is_empty(),
        "an authorisation is not payment"
    );

    admin_order::capture_payment(
        &mut tx,
        &ctx,
        held,
        CapturePayment {
            amount: try_(dec!(100.00)),
            metadata: None,
        },
    )
    .await
    .expect("the money");

    let held_rights = route::list_order_entitlements(&mut tx, &ctx, placed.id)
        .await
        .expect("what the order bought");
    assert_eq!(held_rights.len(), 1);
    assert!(held_rights[0].revoked_at.is_none());

    admin_order::refund_payment(
        &mut tx,
        &ctx,
        held,
        RefundPayment {
            amount: try_(dec!(100.00)),
            reason_id: None,
            note: None,
        },
    )
    .await
    .expect("the money back");

    let after = route::list_order_entitlements(&mut tx, &ctx, placed.id)
        .await
        .expect("what the order still holds");
    assert!(
        after[0].revoked_at.is_some(),
        "the money went back and the file did not"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A shopper's own library, link and download, and a stranger refused at the
/// same three doors.
#[tokio::test]
async fn a_shopper_lists_asks_for_a_link_and_spends_one_download() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    seed_currency(&mut tx, shop.here).await;

    let staff = shop.ctx();
    let buyer = customer::create(&mut tx, &staff, NewCustomer::guest())
        .await
        .expect("a customer")
        .id;

    let variant = a_sellable_file(&mut tx, &staff).await;
    let placed =
        admin_order::create_order(&mut tx, &staff, an_order(variant, Some(buyer), dec!(40.00)))
            .await
            .expect("an order");
    let held = a_held_payment(&mut tx, &staff, shop.here, placed.id, money(dec!(40.00))).await;

    admin_order::capture_payment(
        &mut tx,
        &staff,
        held,
        CapturePayment {
            amount: try_(dec!(40.00)),
            metadata: None,
        },
    )
    .await
    .expect("the money");

    let host = OnlyMine {
        customer: buyer.as_uuid(),
    };
    let shopper = shop.ctx_as(
        Actor::Customer {
            id: buyer.as_uuid(),
        },
        &host as &dyn Host,
    );

    let library = route::my_entitlements(&mut tx, &shopper, route::List::default())
        .await
        .expect("my library");
    assert_eq!(library.len(), 1);

    let link = route::create_token(&mut tx, &shopper, library.items[0].id)
        .await
        .expect("a link");
    assert!(!link.token.is_empty());

    let taken = route::redeem(
        &mut tx,
        &shopper,
        route::Redeem {
            token: link.token.clone(),
            ip: Some("203.0.113.7".into()),
            user_agent: Some("a test".into()),
        },
    )
    .await
    .expect("the download");
    assert_eq!(taken.content_key, "books/novel.epub");
    assert_eq!(taken.remaining, Some(1));

    // Somebody else holding the same link is nobody: the storefront answers
    // for the shopper who is signed in, not for whoever has the string.
    let stranger_host = OnlyMine {
        customer: Uuid::now_v7(),
    };
    let stranger = shop.ctx_as(
        Actor::Customer {
            id: stranger_host.customer,
        },
        &stranger_host as &dyn Host,
    );
    let refused = route::redeem(
        &mut tx,
        &stranger,
        route::Redeem {
            token: link.token,
            ip: None,
            user_agent: None,
        },
    )
    .await
    .expect_err("a link somebody else was sent");
    assert!(refused.is_denied() || refused.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
