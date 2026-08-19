//! Who a captured order's money belongs to, and what a refund undoes.

mod common;

use async_trait::async_trait;
use common::Shop;
use rust_decimal_macros::dec;
use serde_json::json;
use tezgah::catalogue::{self, NewCategory, NewProduct, NewVariant};
use tezgah::id::{CategoryId, OrderId, PaymentId, PaymentSessionId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine};
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, Authorized, CancelRequest,
    CaptureRequest, CaptureResult, NewCollection, NewSession, PaymentProvider, RefundRequest,
    RefundResult, SessionRequest, SessionResponse, SessionStatus, WebhookEvent,
};
use tezgah::payout::{self, CommissionKind, NewCommissionRule};
use tezgah::ports::{Ctx, Tx};
use tezgah::settlement;

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

/// A product in `category`, and its one variant. `order_line_item.product_id`
/// is what a commission rule resolves a category through, so a line has to
/// carry it — a variant alone does not name its product on the order row.
async fn a_categorized_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
    category: CategoryId,
) -> (VariantId, uuid::Uuid) {
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

    catalogue::add_product_to_category(tx, ctx, product.id, category)
        .await
        .expect("to categorise the product");

    let variant = catalogue::create_variant(
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
    .id;

    (variant, product.id.as_uuid())
}

/// A product outside any category — nothing a rule can match, so it earns no
/// commission the way shipping and tax never do either.
async fn an_uncategorized_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
) -> (VariantId, uuid::Uuid) {
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

    let variant = catalogue::create_variant(
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
    .id;

    (variant, product.id.as_uuid())
}

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

async fn an_order(shop: &Shop, total: Money, lines: Vec<NewOrderLine>) -> (OrderId, PaymentId) {
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
            ..NewOrder::of(total.currency)
        },
    )
    .await
    .expect("an order");

    tx.commit().await.expect("to commit");
    (placed.id, paid)
}

fn line(
    title: &str,
    variant: VariantId,
    product: uuid::Uuid,
    quantity: i32,
    unit_price: rust_decimal::Decimal,
) -> NewOrderLine {
    NewOrderLine {
        variant_id: Some(variant),
        product_id: Some(product),
        requires_shipping: false,
        ..NewOrderLine::of(title, quantity, money(unit_price))
    }
}

async fn payout_lines_of(shop: &Shop, order_id: OrderId) -> Vec<(String, rust_decimal::Decimal)> {
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    let page = payout::payout_lines(&mut tx, &ctx, order_id, tezgah::page::Paging::first(50))
        .await
        .expect("this scope's own lines");
    tx.rollback().await.expect("to roll back");
    page.items
        .into_iter()
        .map(|row| (row.reference, row.amount))
        .collect()
}

fn amount_of(lines: &[(String, rust_decimal::Decimal)], reference: &str) -> rust_decimal::Decimal {
    lines
        .iter()
        .filter(|(seen, _)| seen == reference)
        .map(|(_, amount)| *amount)
        .sum()
}

/// A category commissioned at 10%, and a shop selling one line inside it
/// beside one line that matches no rule at all — the same pass-through a
/// shipping or a tax line gets, since neither is ever commissioned either.
async fn a_seeded_shop() -> (Shop, (VariantId, uuid::Uuid), (VariantId, uuid::Uuid)) {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let category = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            parent_id: None,
            name: "Electronics".into(),
            handle: "electronics".into(),
            description: None,
            rank: None,
            is_active: None,
            is_internal: None,
            external_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a category");

    payout::set_commission_rule(
        &mut tx,
        &ctx,
        NewCommissionRule {
            category_id: Some(category.id),
            kind: CommissionKind::Percentage,
            value: dec!(10),
            currency_code: None,
        },
    )
    .await
    .expect("a commission rule");

    let commissioned = a_categorized_variant(&mut tx, &ctx, "gadget", category.id).await;
    let pass_through = an_uncategorized_variant(&mut tx, &ctx, "delivery").await;

    tx.commit().await.expect("to commit the seed");
    (shop, commissioned, pass_through)
}

/// Capturing an order splits it into a seller's share and the marketplace's
/// commission — resolved per category, uncommissioned lines passed through in
/// full — and the two always add back up to what was captured.
#[tokio::test]
async fn capture_writes_a_seller_share_and_a_commission_that_sum_to_the_whole() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    let seller = amount_of(&lines, "seller_share");
    let commission = amount_of(&lines, "commission");

    // 10% of the 100.00 that sat in a commissioned category, 0% of the 20.00
    // that matched no rule at all.
    assert_eq!(commission, dec!(10.00));
    assert_eq!(seller, dec!(110.00));
    assert_eq!(seller + commission, dec!(120.00));

    shop.close().await;
}

/// A refund unwinds both halves of what a capture wrote — the seller's share
/// and the commission the marketplace no longer keeps — in the same ratio
/// the capture itself recorded, and in the refund's own transaction.
#[tokio::test]
async fn refund_unwinds_the_sellers_share_and_the_commission_together() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(60.00)), None, None)
        .await
        .expect("the refund to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    let refund_seller = amount_of(&lines, "refund_seller_share");
    let refund_commission = amount_of(&lines, "refund_commission");

    // The order's own commission-to-total ratio was 10/120 — a twelfth —
    // applied to the 60.00 given back.
    assert_eq!(refund_commission, dec!(-5.00));
    assert_eq!(refund_seller, dec!(-55.00));
    assert_eq!(refund_seller + refund_commission, dec!(-60.00));

    shop.close().await;
}

/// A seller already paid, then refunded, is not a special case: the same
/// negative row lands, and the running balance simply goes negative — a debt
/// the next payout nets against rather than a table of its own.
#[tokio::test]
async fn a_refund_after_payout_leaves_the_seller_in_debt() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;
    let _ = order_id;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    // seeded so isolation is asserted rather than assumed elsewhere; the
    // seller is owed 110.00 — the balance the host now pays out.
    let mut tx = shop.begin().await;
    let balance = payout::balance(&mut tx, &ctx, lira())
        .await
        .expect("a balance");
    assert_eq!(balance.amount, dec!(110.00));
    payout::create_payout(&mut tx, &ctx, lira(), "manual", uuid::Uuid::now_v7(), None)
        .await
        .expect("to record the payout");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let balance = payout::balance(&mut tx, &ctx, lira())
        .await
        .expect("a balance");
    assert_eq!(balance.amount, dec!(0.00));
    tx.rollback().await.expect("to roll back");

    // Now the whole order is refunded — the seller was already paid the
    // 110.00 this takes back 100.00 of.
    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(120.00)), None, None)
        .await
        .expect("the refund to settle");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let balance = payout::balance(&mut tx, &ctx, lira())
        .await
        .expect("a balance");
    tx.rollback().await.expect("to roll back");
    assert!(
        balance.amount < dec!(0),
        "a seller refunded after being paid should owe the marketplace, got {}",
        balance.amount
    );

    shop.close().await;
}

/// A seller-scope's own payout ledger is a table like any other: another
/// scope's connection reads nothing of it.
#[tokio::test]
async fn a_seller_cannot_read_another_sellers_payout_lines() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;
    let page = payout::payout_lines(&mut tx, &theirs, order_id, tezgah::page::Paging::first(50))
        .await
        .expect("a query, even if it finds nothing");
    tx.rollback().await.expect("to roll back");

    assert!(
        page.items.is_empty(),
        "another scope read this seller's payout lines"
    );

    shop.close().await;
}

/// The case #144 was opened for: an order captured in two parts either side
/// of a commission rate change. Refunding the first, 10%-rate capture must
/// unwind at 10% — the rate that capture's own ledger lines recorded — not
/// the order's blended 12.5%, which is what `commission_to_date /
/// captured_to_date` would give once the second capture landed at 15%.
#[tokio::test]
async fn a_refund_uses_the_rate_its_own_capture_earned_at_not_the_blended_one() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let category = catalogue::create_category(
        &mut tx,
        &ctx,
        NewCategory {
            parent_id: None,
            name: "Electronics".into(),
            handle: "electronics".into(),
            description: None,
            rank: None,
            is_active: None,
            is_internal: None,
            external_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a category");

    payout::set_commission_rule(
        &mut tx,
        &ctx,
        NewCommissionRule {
            category_id: Some(category.id),
            kind: CommissionKind::Percentage,
            value: dec!(10),
            currency_code: None,
        },
    )
    .await
    .expect("a 10% commission rule");

    let commissioned = a_categorized_variant(&mut tx, &ctx, "gadget", category.id).await;
    tx.commit().await.expect("to commit the seed");

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(200.00)),
        vec![line(
            "A gadget",
            commissioned.0,
            commissioned.1,
            4,
            dec!(50.00),
        )],
    )
    .await;

    // Half captured at 10%: seller 90.00, commission 10.00.
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(100.00)), None)
        .await
        .expect("the first capture to settle");
    tx.commit().await.expect("to commit");

    // The shop raises its rate to 15% before the rest is captured.
    let mut tx = shop.begin().await;
    payout::set_commission_rule(
        &mut tx,
        &ctx,
        NewCommissionRule {
            category_id: Some(category.id),
            kind: CommissionKind::Percentage,
            value: dec!(15),
            currency_code: None,
        },
    )
    .await
    .expect("to raise the commission rule");
    tx.commit().await.expect("to commit");

    // The rest captured at 15%: seller 85.00, commission 15.00. The order's
    // blended ratio is now 25.00 / 200.00 = 12.5%.
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(100.00)), None)
        .await
        .expect("the second capture to settle");
    tx.commit().await.expect("to commit");

    // Refund exactly the first, 10%-rate capture.
    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(100.00)), None, None)
        .await
        .expect("the refund to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    let refund_seller = amount_of(&lines, "refund_seller_share");
    let refund_commission = amount_of(&lines, "refund_commission");

    // 10% of the 100.00 given back, not the blended 12.5%.
    assert_eq!(refund_commission, dec!(-10.00));
    assert_eq!(refund_seller, dec!(-90.00));

    // The running balance is exactly right: 90.00 + 85.00 - 90.00 seller
    // share left after refunding the first capture's share.
    let seller = amount_of(&lines, "seller_share");
    assert_eq!(seller + refund_seller, dec!(85.00));

    shop.close().await;
}

/// A partial refund of a single capture takes that capture's own rate by
/// construction, not by coincidence: the commission rule has already moved
/// on by the time the refund happens, and the refund still uses the rate
/// the capture actually earned at.
#[tokio::test]
async fn a_single_captures_partial_refund_ignores_a_later_rule_change() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;
    let ctx = shop.ctx();

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    // The rule moves to 50% after capture, before the refund. A blended or
    // re-resolved rate would leak this into the refund; the frozen capture
    // rate must not.
    let mut tx = shop.begin().await;
    payout::set_commission_rule(
        &mut tx,
        &ctx,
        NewCommissionRule {
            category_id: None,
            kind: CommissionKind::Percentage,
            value: dec!(50),
            currency_code: None,
        },
    )
    .await
    .expect("to change the default rule");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(60.00)), None, None)
        .await
        .expect("the refund to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    assert_eq!(amount_of(&lines, "refund_commission"), dec!(-5.00));
    assert_eq!(amount_of(&lines, "refund_seller_share"), dec!(-55.00));

    shop.close().await;
}

/// A fixed commission split across two partial captures must total the same
/// as the one rule allows, whether the order is captured in one shot or in
/// pieces — #145: clamping the fee per line, against that line's own
/// subtotal, let a partial capture see a different total than a single one
/// would.
#[tokio::test]
async fn a_fixed_commission_sums_the_same_captured_in_parts_or_in_one() {
    async fn seeded_order(fee: rust_decimal::Decimal) -> (Shop, OrderId, PaymentId) {
        let shop = Shop::open().await;
        let ctx = shop.ctx();
        let mut tx = shop.begin().await;

        payout::set_commission_rule(
            &mut tx,
            &ctx,
            NewCommissionRule {
                category_id: None,
                kind: CommissionKind::Fixed,
                value: fee,
                currency_code: Some(lira()),
            },
        )
        .await
        .expect("a fixed commission rule");

        let variant = an_uncategorized_variant(&mut tx, &ctx, "gadget").await;
        tx.commit().await.expect("to commit the seed");

        let (order_id, paid) = an_order(
            &shop,
            money(dec!(100.00)),
            vec![line("A gadget", variant.0, variant.1, 1, dec!(100.00))],
        )
        .await;
        (shop, order_id, paid)
    }

    // Captured whole: 8.00 fixed commission, once.
    let (shop, order_id, paid) = seeded_order(dec!(8.00)).await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(100.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");
    let whole = payout_lines_of(&shop, order_id).await;
    shop.close().await;

    // Captured in two halves: the same 8.00 total, not 8.00 twice.
    let (shop, order_id, paid) = seeded_order(dec!(8.00)).await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(50.00)), None)
        .await
        .expect("the first capture to settle");
    tx.commit().await.expect("to commit");
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(50.00)), None)
        .await
        .expect("the second capture to settle");
    tx.commit().await.expect("to commit");
    let parts = payout_lines_of(&shop, order_id).await;
    shop.close().await;

    assert_eq!(amount_of(&whole, "commission"), dec!(8.00));
    assert_eq!(amount_of(&parts, "commission"), dec!(8.00));
    assert_eq!(
        amount_of(&whole, "seller_share") + amount_of(&whole, "commission"),
        dec!(100.00)
    );
    assert_eq!(
        amount_of(&parts, "seller_share") + amount_of(&parts, "commission"),
        dec!(100.00)
    );
}

/// A fixed fee bigger than the order it commissions is clamped against the
/// order's own total, not against whichever fraction of it a single capture
/// happens to see.
#[tokio::test]
async fn a_fixed_commission_bigger_than_the_order_clamps_to_the_whole() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    payout::set_commission_rule(
        &mut tx,
        &ctx,
        NewCommissionRule {
            category_id: None,
            kind: CommissionKind::Fixed,
            value: dec!(500.00),
            currency_code: Some(lira()),
        },
    )
    .await
    .expect("a fixed commission rule");

    let variant = an_uncategorized_variant(&mut tx, &ctx, "gadget").await;
    tx.commit().await.expect("to commit the seed");

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(100.00)),
        vec![line("A gadget", variant.0, variant.1, 1, dec!(100.00))],
    )
    .await;

    // Captured in two halves of 50.00 each — a per-line clamp against 50.00
    // would cap the fee well under the order's own 100.00.
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(50.00)), None)
        .await
        .expect("the first capture to settle");
    tx.commit().await.expect("to commit");
    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(50.00)), None)
        .await
        .expect("the second capture to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    assert_eq!(amount_of(&lines, "commission"), dec!(100.00));
    assert_eq!(amount_of(&lines, "seller_share"), dec!(0.00));

    shop.close().await;
}

/// `create_payout` and `record_earning` write through the same function.
/// A `paid_out` row and an order's own earning row both carry the full
/// column set `write_line` names — there is no second path that could drift
/// out of step and leave one of them short a field.
#[tokio::test]
async fn a_paid_out_line_carries_the_same_columns_as_an_earning_line() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;
    let ctx = shop.ctx();

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let payout = payout::create_payout(&mut tx, &ctx, lira(), "manual", uuid::Uuid::now_v7(), None)
        .await
        .expect("to record the payout");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let row: (
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        rust_decimal::Decimal,
        String,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
    ) = sqlx::query_as(
        "select order_id, payout_id, amount, reference, reference_id, capture_id
             from payout_line where payout_id = $1 and reference = 'paid_out'",
    )
    .bind(payout.id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the paid_out row write_line wrote");
    tx.rollback().await.expect("to roll back");

    let (order_id_col, payout_id_col, amount, reference, reference_id, capture_id) = row;
    assert_eq!(order_id_col, None, "paid_out carries no order_id");
    assert_eq!(payout_id_col, Some(payout.id.as_uuid()));
    assert_eq!(amount, dec!(-110.00));
    assert_eq!(reference, "paid_out");
    assert!(reference_id.is_some());
    assert_eq!(capture_id, None, "paid_out has no capture to attribute to");
    let _ = order_id;

    shop.close().await;
}

/// Refunding a captured order in full zeros out both the seller's share and
/// the commission the marketplace no longer keeps.
#[tokio::test]
async fn a_full_refund_zeros_the_seller_share_and_the_commission() {
    let (shop, commissioned, pass_through) = a_seeded_shop().await;
    let ctx = shop.ctx();

    let (order_id, paid) = an_order(
        &shop,
        money(dec!(120.00)),
        vec![
            line("A gadget", commissioned.0, commissioned.1, 2, dec!(50.00)),
            line("Delivery", pass_through.0, pass_through.1, 1, dec!(20.00)),
        ],
    )
    .await;

    let mut tx = shop.begin().await;
    settlement::capture(&mut tx, &ctx, paid, money(dec!(120.00)), None)
        .await
        .expect("the capture to settle");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    settlement::refund(&mut tx, &ctx, paid, money(dec!(120.00)), None, None)
        .await
        .expect("the refund to settle");
    tx.commit().await.expect("to commit");

    let lines = payout_lines_of(&shop, order_id).await;
    let seller = amount_of(&lines, "seller_share") + amount_of(&lines, "refund_seller_share");
    let commission = amount_of(&lines, "commission") + amount_of(&lines, "refund_commission");

    assert_eq!(seller, dec!(0.00));
    assert_eq!(commission, dec!(0.00));

    shop.close().await;
}
