//! Agreements, withdrawal and invoices, from the route to the row.
//!
//! Five events that never reached a sink and an invoice status that could not
//! be set, because nothing called any of it. The other half of the same issue
//! is here too: `withdrawal_exclusion` was hard-coded `None` everywhere, so a
//! shop selling a sealed or a personalised thing recorded a right of
//! withdrawal it does not owe. It now comes off the variant, and the checkout
//! copies it onto the line the way it copies the price.

mod common;

use std::sync::Arc;

use common::{OnlyMine, Shop, Teller};
use rust_decimal_macros::dec;
use tezgah::api::admin_catalogue;
use tezgah::api::agreement as route;
use tezgah::checkout::Checkout;
use tezgah::id::{CustomerId, OrderId};
use tezgah::payment::PaymentProvider;
use tezgah::ports::{Actor, Ctx, Host};

async fn an_order(shop: &Shop, here: &common::Shelf) -> OrderId {
    let checkout = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id)
        .await
        .expect("a checkout")
        .order_id
        .expect("an order")
}

#[tokio::test]
async fn a_sealed_thing_is_sold_without_a_right_to_send_it_back() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let variant = admin_catalogue::update_variant(
        &mut tx,
        &ctx,
        here.variant_id,
        admin_catalogue::UpdateVariant {
            withdrawal_exclusion: Some(Some("hygiene".into())),
            ..admin_catalogue::UpdateVariant::default()
        },
    )
    .await
    .expect("the variant to say why");
    assert_eq!(variant.withdrawal_exclusion.as_deref(), Some("hygiene"));
    tx.commit().await.expect("to commit");

    let order_id = an_order(&shop, &here).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let windows = route::withdrawal_windows(&mut tx, &ctx, order_id)
        .await
        .expect("the windows");
    let line = windows.first().expect("a line");
    assert!(!line.eligible, "a sealed line kept a right it does not owe");
    assert_eq!(line.exclusion_reason.as_deref(), Some("hygiene"));
    assert_eq!(line.deadline, None);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn an_ordinary_thing_keeps_its_right_of_withdrawal() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let order_id = an_order(&shop, &here).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let windows = route::withdrawal_windows(&mut tx, &ctx, order_id)
        .await
        .expect("the windows");
    let line = windows.first().expect("a line");
    assert!(line.eligible);
    assert_eq!(line.exclusion_reason, None);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn an_order_carries_the_text_its_buyer_accepted() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;
    let order_id = an_order(&shop, &here).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let published = route::publish_agreement(
        &mut tx,
        &ctx,
        route::PublishAgreement {
            kind: "distance_sale".into(),
            locale: "tr".into(),
            body: "The terms as they read today.".into(),
            effective_from: None,
            metadata: None,
        },
    )
    .await
    .expect("a published version");

    let listed = route::list_agreements(&mut tx, &ctx, route::ListAgreements::default())
        .await
        .expect("the versions");
    assert_eq!(listed.items.len(), 1);

    let accepted = route::accept_agreement(
        &mut tx,
        &ctx,
        order_id,
        route::AcceptAgreement {
            agreement_version_id: published.id,
            accepted_at: None,
            ip: Some("203.0.113.7".into()),
            user_agent: Some("a test".into()),
            metadata: None,
        },
    )
    .await
    .expect("the acceptance");
    assert_eq!(accepted.body_hash, published.body_hash);

    let again = route::accept_agreement(
        &mut tx,
        &ctx,
        order_id,
        route::AcceptAgreement {
            agreement_version_id: published.id,
            accepted_at: None,
            ip: None,
            user_agent: None,
            metadata: None,
        },
    )
    .await
    .expect_err("one document of one kind per order");
    assert!(again.is_conflict());

    // Publishing again does not change what this order accepted.
    route::publish_agreement(
        &mut tx,
        &ctx,
        route::PublishAgreement {
            kind: "distance_sale".into(),
            locale: "tr".into(),
            body: "The terms as they read tomorrow.".into(),
            effective_from: None,
            metadata: None,
        },
    )
    .await
    .expect("a second version");

    let text = route::accepted_text(&mut tx, &ctx, order_id, "distance_sale")
        .await
        .expect("the text they read");
    assert_eq!(text.body, "The terms as they read today.");

    let held = route::order_agreements(&mut tx, &ctx, order_id)
        .await
        .expect("what the order accepted");
    assert_eq!(held.len(), 1);

    assert!(
        shop.host.emitted("order.agreement_accepted"),
        "the acceptance reached no event sink"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn an_invoice_is_recorded_and_moves_to_issued() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;
    let order_id = an_order(&shop, &here).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let invoice = |number: &str, status: &str| route::RecordInvoice {
        number: number.into(),
        external_id: Some(format!("ettn-{number}")),
        provider: Some("an integrator".into()),
        status: status.into(),
        total: dec!(20),
        currency_code: "TRY".into(),
        issued_at: None,
        document_url: None,
        metadata: None,
    };

    let recorded = route::record_invoice(&mut tx, &ctx, order_id, invoice("A-1", "requested"))
        .await
        .expect("an invoice");
    assert_eq!(recorded.status, "requested");

    let twice = route::record_invoice(&mut tx, &ctx, order_id, invoice("A-1", "requested"))
        .await
        .expect_err("one serial is one invoice");
    assert!(twice.is_conflict());

    let issued = route::set_invoice_status(
        &mut tx,
        &ctx,
        recorded.id,
        route::SetInvoiceStatus {
            status: "issued".into(),
        },
    )
    .await
    .expect("the authority's answer");
    assert_eq!(issued.status, "issued");

    let note = route::record_credit_note(
        &mut tx,
        &ctx,
        order_id,
        recorded.id,
        invoice("A-1-C", "issued"),
    )
    .await
    .expect("a credit note");
    assert_eq!(note.replaces_invoice_id, Some(recorded.id));

    let all = route::list_invoices(&mut tx, &ctx, order_id)
        .await
        .expect("the documents");
    assert_eq!(all.len(), 2);

    assert!(shop.host.emitted("order.invoice_recorded"));
    assert!(shop.host.emitted("order.invoice_status_changed"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn another_customers_order_is_not_mine_to_read_or_sign() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let owner = common::a_customer(&mut tx, &ctx).await;
    sqlx::query("update cart set customer_id = $3 where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(here.cart_id.as_uuid())
        .bind(owner.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the cart to belong to somebody");

    let published = route::publish_agreement(
        &mut tx,
        &ctx,
        route::PublishAgreement {
            kind: "pre_contract".into(),
            locale: "tr".into(),
            body: "What you are told before you buy.".into(),
            effective_from: None,
            metadata: None,
        },
    )
    .await
    .expect("a published version");
    tx.commit().await.expect("to commit");

    let order_id = an_order(&shop, &here).await;

    let stranger = CustomerId::new();
    let host = OnlyMine {
        customer: stranger.as_uuid(),
    };
    let theirs = Ctx::new(
        shop.here,
        Actor::Customer {
            id: stranger.as_uuid(),
        },
        &host as &dyn Host,
    );

    let mut tx = shop.begin().await;

    let refused = route::accept_agreement(
        &mut tx,
        &theirs,
        order_id,
        route::AcceptAgreement {
            agreement_version_id: published.id,
            accepted_at: None,
            ip: None,
            user_agent: None,
            metadata: None,
        },
    )
    .await
    .expect_err("somebody else's order");
    assert!(refused.is_denied());

    let refused = route::accepted_text(&mut tx, &theirs, order_id, "pre_contract")
        .await
        .expect_err("somebody else's order");
    assert!(refused.is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
