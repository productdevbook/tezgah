//! Promotion, against a real Postgres.
//!
//! The two that matter: an amount shared across lines has to add back up to
//! the amount, and a coupon with one use left has to go to exactly one of two
//! people asking for it at the same time.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::id::{CartId, PromotionId};
use tezgah::money::Currency;
use tezgah::ports::{Ctx, Tx};
use tezgah::promotion::{
    self, Allocation, MethodKind, NewApplicationMethod, NewPromotion, PromotionKind, Status,
    TargetKind,
};
use uuid::Uuid;

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

async fn seed_currency(tx: &mut Tx<'_>, scope: uuid::Uuid) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, '₺', '₺', 'Turkish lira')",
    )
    .bind(Uuid::now_v7())
    .bind(scope)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

/// A cart with `lines` lines, each one item at ten lira.
async fn a_cart(tx: &mut Tx<'_>, scope: uuid::Uuid, lines: usize) -> CartId {
    let cart = CartId::new();
    sqlx::query("insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')")
        .bind(cart.as_uuid())
        .bind(scope)
        .execute(&mut **tx)
        .await
        .expect("a cart");

    for at in 0..lines {
        sqlx::query(
            "insert into cart_line_item
                 (id, scope, cart_id, product_title, quantity, unit_price, currency_code)
             values ($1, $2, $3, $4, 1, 10, 'TRY')",
        )
        .bind(Uuid::now_v7())
        .bind(scope)
        .bind(cart.as_uuid())
        .bind(format!("Item {at}"))
        .execute(&mut **tx)
        .await
        .expect("a line item");
    }

    cart
}

async fn a_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    code: &str,
    usage_limit: Option<i32>,
) -> PromotionId {
    let promotion = promotion::create_promotion(
        tx,
        ctx,
        NewPromotion {
            code: code.into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: true,
            campaign_id: None,
            usage_limit,
            customer_usage_limit: None,
        },
    )
    .await
    .expect("a promotion");

    promotion.id
}

#[tokio::test]
async fn an_amount_shared_across_lines_adds_back_up_to_the_amount() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let cart = a_cart(&mut tx, shop.here.0, 3).await;
    let id = a_promotion(&mut tx, &ctx, "TENOFF", None).await;

    promotion::set_application_method(
        &mut tx,
        &ctx,
        NewApplicationMethod {
            promotion_id: id,
            kind: MethodKind::Fixed,
            target_type: TargetKind::Order,
            allocation: Some(Allocation::Across),
            value: dec!(10),
            currency_code: Some(lira()),
            max_quantity: None,
            apply_to_quantity: None,
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect("an application method");

    let taken = promotion::apply(&mut tx, &ctx, cart)
        .await
        .expect("a discount");

    assert_eq!(taken.len(), 3, "an order-wide discount reaches every line");
    let total: Decimal = taken.iter().map(|one| one.amount.amount).sum();
    assert_eq!(total, dec!(10.00), "a rounded line lost a kuruş");

    let stored: Decimal = sqlx::query_scalar(
        "select coalesce(sum(a.amount), 0)
         from cart_line_item_adjustment a
         join cart_line_item l on l.id = a.cart_line_item_id and l.scope = a.scope
         where a.scope = $1 and l.cart_id = $2",
    )
    .bind(shop.here.0)
    .bind(cart.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the stored adjustments");
    assert_eq!(stored, dec!(10.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_percentage_is_taken_off_each_line_it_lands_on() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let cart = a_cart(&mut tx, shop.here.0, 2).await;
    let id = a_promotion(&mut tx, &ctx, "TENPC", None).await;

    promotion::set_application_method(
        &mut tx,
        &ctx,
        NewApplicationMethod {
            promotion_id: id,
            kind: MethodKind::Percentage,
            target_type: TargetKind::Items,
            allocation: Some(Allocation::Each),
            value: dec!(10),
            currency_code: None,
            max_quantity: None,
            apply_to_quantity: None,
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect("an application method");

    let taken = promotion::apply(&mut tx, &ctx, cart)
        .await
        .expect("a discount");

    assert_eq!(taken.len(), 2);
    assert!(taken.iter().all(|one| one.amount.amount == dec!(1.00)));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn two_people_reaching_for_the_last_use_of_a_coupon_and_one_of_them_getting_it() {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    let id = a_promotion(&mut setup, &shop.ctx(), "LASTONE", Some(1)).await;
    setup.commit().await.expect("to keep the promotion");

    let claim = || async {
        let mut tx = shop.begin().await;
        let taken = promotion::claim(&mut tx, &shop.ctx(), id, None).await;
        match taken {
            Ok(()) => {
                tx.commit().await.expect("to keep the claim");
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.expect("to give it back");
                Err(err)
            }
        }
    };

    // Two connections at the same moment, not one after the other.
    let (first, second) = tokio::join!(claim(), claim());

    assert_eq!(
        i32::from(first.is_ok()) + i32::from(second.is_ok()),
        1,
        "a coupon with one use left went to both of them or to neither"
    );

    let used: i32 = sqlx::query_scalar("select used from promotion where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(id.as_uuid())
        .fetch_one(&shop.pool)
        .await
        .expect("the counter");
    assert_eq!(used, 1);

    shop.close().await;
}

#[tokio::test]
async fn a_customer_with_one_claim_each_cannot_take_a_second() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let customer = tezgah::id::CustomerId::new();

    let mut tx = shop.begin().await;
    let promotion = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: "ONCE".into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: false,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: Some(1),
        },
    )
    .await
    .expect("a promotion");

    promotion::claim(&mut tx, &ctx, promotion.id, Some(customer))
        .await
        .expect("the first claim");

    let refused = promotion::claim(&mut tx, &ctx, promotion.id, Some(customer))
        .await
        .expect_err("the second claim");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn one_shops_promotions_are_invisible_to_another() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    seed_currency(&mut mine, shop.here.0).await;
    let cart = a_cart(&mut mine, shop.here.0, 1).await;
    a_promotion(&mut mine, &shop.ctx(), "SECRET", None).await;
    mine.commit().await.expect("to keep them");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let seen = promotion::promotions(&mut theirs, &shop.theirs(), tezgah::Paging::first(10))
        .await
        .expect("a page");
    assert!(seen.is_empty());

    let missing = promotion::apply(&mut theirs, &shop.theirs(), cart)
        .await
        .expect_err("another shop's cart");
    assert!(missing.is_not_found());

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
}
