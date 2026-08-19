//! Promotion, against a real Postgres.
//!
//! The two that matter: an amount shared across lines has to add back up to
//! the amount, and a coupon with one use left has to go to exactly one of two
//! people asking for it at the same time.

mod common;

use common::{Doorman, Shop};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::id::{CartId, PromotionId};
use tezgah::money::{Currency, Money};
use tezgah::ports::{Actor, Ctx, Tx};
use tezgah::promotion::{
    self, Allocation, BudgetKind, MethodKind, NewApplicationMethod, NewCampaign, NewCampaignBudget,
    NewPromotion, PromotionKind, Status, TargetKind,
};
use uuid::Uuid;

fn nothing() -> Money {
    Money::new(Decimal::ZERO, lira())
}

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

async fn seed_currency(tx: &mut Tx<'_>, scope: uuid::Uuid) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, '₺', '₺', 'Turkish lira')
         on conflict do nothing",
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

/// A cart with one line per price in `prices`, each a single unit.
async fn a_cart_with_prices(tx: &mut Tx<'_>, scope: uuid::Uuid, prices: &[Decimal]) -> CartId {
    let cart = CartId::new();
    sqlx::query("insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')")
        .bind(cart.as_uuid())
        .bind(scope)
        .execute(&mut **tx)
        .await
        .expect("a cart");

    for (at, price) in prices.iter().enumerate() {
        sqlx::query(
            "insert into cart_line_item
                 (id, scope, cart_id, product_title, quantity, unit_price, currency_code)
             values ($1, $2, $3, $4, 1, $5, 'TRY')",
        )
        .bind(Uuid::now_v7())
        .bind(scope)
        .bind(cart.as_uuid())
        .bind(format!("Item {at}"))
        .bind(price)
        .execute(&mut **tx)
        .await
        .expect("a line item");
    }

    cart
}

/// Puts a code on a cart the way a customer typing it would, without going
/// through checkout's own path to get there.
async fn with_code(tx: &mut Tx<'_>, scope: uuid::Uuid, cart: CartId, code: &str) {
    sqlx::query(
        "update cart set metadata = jsonb_build_object('promotions', jsonb_build_array($3::text))
         where scope = $1 and id = $2",
    )
    .bind(scope)
    .bind(cart.as_uuid())
    .bind(code)
    .execute(&mut **tx)
    .await
    .expect("a promo code on the cart");
}

/// Every line an adjustment landed on, as `(unit_price, amount)`, sorted so two
/// runs over differently-ordered rows compare equal.
async fn adjusted_lines(
    tx: &mut Tx<'_>,
    scope: uuid::Uuid,
    cart: CartId,
) -> Vec<(Decimal, Decimal)> {
    let mut rows: Vec<(Decimal, Decimal)> = sqlx::query_as(
        "select l.unit_price, a.amount
         from cart_line_item_adjustment a
         join cart_line_item l on l.id = a.cart_line_item_id and l.scope = a.scope
         where a.scope = $1 and l.cart_id = $2",
    )
    .bind(scope)
    .bind(cart.as_uuid())
    .fetch_all(&mut **tx)
    .await
    .expect("the stored adjustments");
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    rows
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
            metadata: None,
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
        let taken = promotion::claim(&mut tx, &shop.ctx(), id, None, nothing()).await;
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

    let mut after = shop.begin().await;
    let used: i32 = sqlx::query_scalar("select used from promotion where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(id.as_uuid())
        .fetch_one(&mut *after)
        .await
        .expect("the counter");
    after.commit().await.expect("to finish reading");
    assert_eq!(used, 1);

    shop.close().await;
}

#[tokio::test]
async fn a_customer_with_one_claim_each_cannot_take_a_second() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let customer = common::a_customer(&mut tx, &ctx).await;
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
            metadata: None,
        },
    )
    .await
    .expect("a promotion");

    promotion::claim(&mut tx, &ctx, promotion.id, Some(customer), nothing())
        .await
        .expect("the first claim");

    let refused = promotion::claim(&mut tx, &ctx, promotion.id, Some(customer), nothing())
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

/// A campaign whose budget is money, not uses: the claim charges what it gave
/// away, and the cap binds.
async fn a_campaign_spending(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    identifier: &str,
    cap: Decimal,
) -> tezgah::id::CampaignId {
    let campaign = promotion::create_campaign(
        tx,
        ctx,
        NewCampaign {
            identifier: identifier.into(),
            name: identifier.into(),
            description: None,
            starts_at: None,
            ends_at: None,
            metadata: None,
        },
    )
    .await
    .expect("a campaign");

    promotion::set_campaign_budget(
        tx,
        ctx,
        NewCampaignBudget {
            campaign_id: campaign.id,
            kind: BudgetKind::Spend,
            cap: Some(cap),
            currency_code: Some(lira()),
            attribute: None,
        },
    )
    .await
    .expect("a spend budget");

    campaign.id
}

async fn on_campaign(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    code: &str,
    campaign: tezgah::id::CampaignId,
) -> PromotionId {
    promotion::create_promotion(
        tx,
        ctx,
        NewPromotion {
            code: code.into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: false,
            campaign_id: Some(campaign),
            usage_limit: None,
            customer_usage_limit: None,
            metadata: None,
        },
    )
    .await
    .expect("a promotion")
    .id
}

async fn spent(tx: &mut Tx<'_>, scope: uuid::Uuid, campaign: tezgah::id::CampaignId) -> Decimal {
    sqlx::query_scalar("select used from campaign_budget where scope = $1 and campaign_id = $2")
        .bind(scope)
        .bind(campaign.as_uuid())
        .fetch_one(&mut **tx)
        .await
        .expect("the budget")
}

#[tokio::test]
async fn a_campaigns_money_is_charged_by_what_a_claim_gave_away() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let campaign = a_campaign_spending(&mut tx, &ctx, "SPEND50", dec!(50.00)).await;
    let id = on_campaign(&mut tx, &ctx, "SPENDER", campaign).await;

    promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(30.00), lira()))
        .await
        .expect("the first claim");
    assert_eq!(spent(&mut tx, shop.here.0, campaign).await, dec!(30.00));

    let refused = promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(30.00), lira()))
        .await
        .expect_err("a claim that would overspend the campaign");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn releasing_a_claim_gives_the_money_back_to_the_campaign() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let campaign = a_campaign_spending(&mut tx, &ctx, "SPEND20", dec!(20.00)).await;
    let id = on_campaign(&mut tx, &ctx, "GIVEBACK", campaign).await;

    promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(20.00), lira()))
        .await
        .expect("a claim");
    assert_eq!(spent(&mut tx, shop.here.0, campaign).await, dec!(20.00));

    promotion::release(&mut tx, &ctx, id, None, Money::new(dec!(20.00), lira()))
        .await
        .expect("the release");
    assert_eq!(spent(&mut tx, shop.here.0, campaign).await, Decimal::ZERO);

    promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(20.00), lira()))
        .await
        .expect("the budget to be spendable again");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_campaign_counting_uses_ignores_what_a_claim_gave_away() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let campaign = promotion::create_campaign(
        &mut tx,
        &ctx,
        NewCampaign {
            identifier: "TWICE".into(),
            name: "Twice".into(),
            description: None,
            starts_at: None,
            ends_at: None,
            metadata: None,
        },
    )
    .await
    .expect("a campaign");

    promotion::set_campaign_budget(
        &mut tx,
        &ctx,
        NewCampaignBudget {
            campaign_id: campaign.id,
            kind: BudgetKind::Usage,
            cap: Some(dec!(1)),
            currency_code: None,
            attribute: None,
        },
    )
    .await
    .expect("a usage budget");

    let id = on_campaign(&mut tx, &ctx, "COUNTED", campaign.id).await;

    promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(99.00), lira()))
        .await
        .expect("the one use");
    assert_eq!(spent(&mut tx, shop.here.0, campaign.id).await, dec!(1));

    let refused = promotion::claim(&mut tx, &ctx, id, None, Money::new(dec!(1.00), lira()))
        .await
        .expect_err("the second use");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn another_shop_cannot_spend_this_ones_campaign() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    seed_currency(&mut mine, shop.here.0).await;
    let campaign = a_campaign_spending(&mut mine, &shop.ctx(), "MINE", dec!(50.00)).await;
    let id = on_campaign(&mut mine, &shop.ctx(), "MINEONLY", campaign).await;
    mine.commit().await.expect("to keep them");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let refused = promotion::claim(
        &mut theirs,
        &shop.theirs(),
        id,
        None,
        Money::new(dec!(10.00), lira()),
    )
    .await
    .expect_err("another shop's promotion");
    assert!(refused.is_conflict() || refused.is_not_found());
    theirs.rollback().await.expect("to roll back");

    let mut after = shop.begin().await;
    assert_eq!(
        spent(&mut after, shop.here.0, campaign).await,
        Decimal::ZERO
    );
    after.rollback().await.expect("to roll back");

    shop.close().await;
}

/// #162: `apply` read the cart and only asked once a row existed, so a
/// synthetic id answered `not_found` without anybody being refused.
#[tokio::test]
async fn applying_promotions_to_a_cart_that_does_not_exist_is_denied_not_not_found() {
    let shop = Shop::open().await;
    let doorman = Doorman;
    let refused = shop.ctx_as(Actor::System, &doorman);
    let mut tx = shop.begin().await;

    let denied = promotion::apply(&mut tx, &refused, CartId::new())
        .await
        .expect_err("a refused actor prices nothing for a cart that isn't there");
    assert!(
        denied.is_denied(),
        "a synthetic cart id must be refused, not answered as missing: {:?}",
        denied.code()
    );

    tx.rollback().await.ok();
    shop.close().await;
}

/// #177: a plain promotion's `Allocation::Once` is `buy_get`'s own
/// cheapest-first quota, reached without faking buy rules to get there. Both
/// pick the same units of the same cart for the same money.
#[tokio::test]
async fn once_and_buy_get_discount_the_same_cheapest_units() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;

    // The cheapest two units are the ten and the twenty; the thirty is never
    // among them.
    let prices = [dec!(30), dec!(10), dec!(20)];

    let once_cart = a_cart_with_prices(&mut tx, shop.here.0, &prices).await;
    let once = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: "ONCE10".into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: false,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: None,
            metadata: None,
        },
    )
    .await
    .expect("a promotion")
    .id;
    promotion::set_application_method(
        &mut tx,
        &ctx,
        NewApplicationMethod {
            promotion_id: once,
            kind: MethodKind::Percentage,
            target_type: TargetKind::Items,
            allocation: Some(Allocation::Once),
            value: dec!(10),
            currency_code: None,
            max_quantity: Some(2),
            apply_to_quantity: None,
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect("an application method");
    with_code(&mut tx, shop.here.0, once_cart, "ONCE10").await;

    let buy_get_cart = a_cart_with_prices(&mut tx, shop.here.0, &prices).await;
    let buy_get = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: "BUYGET10".into(),
            kind: PromotionKind::BuyGet,
            status: Status::Active,
            is_automatic: false,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: None,
            metadata: None,
        },
    )
    .await
    .expect("a promotion")
    .id;
    promotion::set_application_method(
        &mut tx,
        &ctx,
        NewApplicationMethod {
            promotion_id: buy_get,
            kind: MethodKind::Percentage,
            target_type: TargetKind::Items,
            allocation: None,
            value: dec!(10),
            currency_code: None,
            max_quantity: None,
            apply_to_quantity: Some(2),
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect("an application method");
    with_code(&mut tx, shop.here.0, buy_get_cart, "BUYGET10").await;

    let once_taken = promotion::apply(&mut tx, &ctx, once_cart)
        .await
        .expect("a discount");
    let buy_get_taken = promotion::apply(&mut tx, &ctx, buy_get_cart)
        .await
        .expect("a discount");

    assert_eq!(once_taken.len(), 2, "the thirty-lira line keeps its price");
    assert_eq!(buy_get_taken.len(), 2);

    let once_total: Decimal = once_taken.iter().map(|one| one.amount.amount).sum();
    let buy_get_total: Decimal = buy_get_taken.iter().map(|one| one.amount.amount).sum();
    assert_eq!(
        once_total,
        dec!(3.00),
        "ten percent of ten plus ten percent of twenty"
    );
    assert_eq!(
        once_total, buy_get_total,
        "a plain promotion's once and buy_get's quota agree"
    );

    let once_lines = adjusted_lines(&mut tx, shop.here.0, once_cart).await;
    let buy_get_lines = adjusted_lines(&mut tx, shop.here.0, buy_get_cart).await;
    assert_eq!(
        once_lines,
        vec![(dec!(10), dec!(1.00)), (dec!(20), dec!(2.00))],
        "the cheapest two units, not the thirty-lira one"
    );
    assert_eq!(
        once_lines, buy_get_lines,
        "the two paths reach the same lines for the same amounts"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// #177: an order-wide discount has no units to sort by price, so `once` is
/// refused for it at the point the application method is set rather than
/// silently discounting the whole order.
#[tokio::test]
async fn once_allocation_is_refused_against_the_whole_order() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let id = a_promotion(&mut tx, &ctx, "ONCEORDER", None).await;

    let denied = promotion::set_application_method(
        &mut tx,
        &ctx,
        NewApplicationMethod {
            promotion_id: id,
            kind: MethodKind::Percentage,
            target_type: TargetKind::Order,
            allocation: Some(Allocation::Once),
            value: dec!(10),
            currency_code: None,
            max_quantity: Some(2),
            apply_to_quantity: None,
            buy_rules_min_quantity: None,
        },
    )
    .await
    .expect_err("once needs units to sort by price, and the order has none");
    assert_eq!(denied.code(), "invalid");

    tx.rollback().await.ok();
    shop.close().await;
}

/// #179: a promotion and its campaign carry their own metadata, the way every
/// other entity in the schema does.
#[tokio::test]
async fn a_promotions_metadata_is_written_and_read_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let created = promotion::create_promotion(
        &mut tx,
        &ctx,
        NewPromotion {
            code: "META".into(),
            kind: PromotionKind::Standard,
            status: Status::Active,
            is_automatic: false,
            campaign_id: None,
            usage_limit: None,
            customer_usage_limit: None,
            metadata: Some(serde_json::json!({ "source": "example.test" })),
        },
    )
    .await
    .expect("a promotion");
    assert_eq!(
        created.metadata,
        Some(serde_json::json!({ "source": "example.test" }))
    );

    let fetched = promotion::promotion(&mut tx, &ctx, created.id)
        .await
        .expect("the promotion back");
    assert_eq!(fetched.metadata, created.metadata);

    let updated = promotion::update_promotion(
        &mut tx,
        &ctx,
        created.id,
        promotion::PromotionPatch {
            metadata: Some(serde_json::json!({ "source": "example.com" })),
            ..Default::default()
        },
    )
    .await
    .expect("to edit the metadata");
    assert_eq!(
        updated.metadata,
        Some(serde_json::json!({ "source": "example.com" }))
    );

    let campaign = promotion::create_campaign(
        &mut tx,
        &ctx,
        NewCampaign {
            identifier: "welcome".into(),
            name: "Welcome".into(),
            description: None,
            starts_at: None,
            ends_at: None,
            metadata: Some(serde_json::json!({ "owner": "marketing" })),
        },
    )
    .await
    .expect("a campaign");
    assert_eq!(
        campaign.metadata,
        Some(serde_json::json!({ "owner": "marketing" }))
    );

    tx.rollback().await.ok();
    shop.close().await;
}

/// #178: a per-customer campaign budget sees every promotion the campaign
/// runs, not just one — the gap `promotion.customer_usage_limit` and
/// `promotion_usage` cannot close, because both are scoped to a single
/// promotion.
#[tokio::test]
async fn a_campaign_budget_by_attribute_counts_every_promotion_it_runs() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let mine = common::a_customer(&mut tx, &ctx).await;
    let someone_elses = common::a_customer(&mut tx, &ctx).await;

    let campaign = promotion::create_campaign(
        &mut tx,
        &ctx,
        NewCampaign {
            identifier: "WELCOME".into(),
            name: "Welcome".into(),
            description: None,
            starts_at: None,
            ends_at: None,
            metadata: None,
        },
    )
    .await
    .expect("a campaign");

    promotion::set_campaign_budget(
        &mut tx,
        &ctx,
        NewCampaignBudget {
            campaign_id: campaign.id,
            kind: BudgetKind::UseByAttribute,
            cap: Some(dec!(2)),
            currency_code: None,
            attribute: Some("customer_id".into()),
        },
    )
    .await
    .expect("a per-customer budget");

    let first = on_campaign(&mut tx, &ctx, "WELCOME-A", campaign.id).await;
    let second = on_campaign(&mut tx, &ctx, "WELCOME-B", campaign.id).await;

    // The cap is two, spread across the campaign's two promotions rather than
    // one each.
    promotion::claim(&mut tx, &ctx, first, Some(mine), nothing())
        .await
        .expect("the first of two");
    promotion::claim(&mut tx, &ctx, second, Some(mine), nothing())
        .await
        .expect("the second of two");

    let refused = promotion::claim(&mut tx, &ctx, first, Some(mine), nothing())
        .await
        .expect_err("a third claim, from either promotion, over the cap");
    assert!(refused.is_conflict());

    // A cap kept per customer never sees another customer's claims.
    promotion::claim(&mut tx, &ctx, first, Some(someone_elses), nothing())
        .await
        .expect("a different customer's own two claims");

    let usage = promotion::campaign_budget_usage(&mut tx, &ctx, campaign.id)
        .await
        .expect("the per-attribute usage");
    let mine_used = usage
        .iter()
        .find(|row| row.attribute_value == mine.to_string())
        .expect("my own row")
        .used;
    assert_eq!(mine_used, dec!(2));

    tx.rollback().await.ok();
    shop.close().await;
}

/// #178: the same concurrency guarantee `campaign_budget`'s own aggregate
/// counter already has, now proven for the per-attribute row — two real
/// connections, at the same moment, not one after the other.
#[tokio::test]
async fn two_orders_claiming_a_customers_last_unit_of_a_campaign_budget_at_once() {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    let ctx = shop.ctx();
    let customer = common::a_customer(&mut setup, &ctx).await;

    let campaign = promotion::create_campaign(
        &mut setup,
        &ctx,
        NewCampaign {
            identifier: "LASTUNIT".into(),
            name: "Last unit".into(),
            description: None,
            starts_at: None,
            ends_at: None,
            metadata: None,
        },
    )
    .await
    .expect("a campaign");

    promotion::set_campaign_budget(
        &mut setup,
        &ctx,
        NewCampaignBudget {
            campaign_id: campaign.id,
            kind: BudgetKind::UseByAttribute,
            cap: Some(dec!(1)),
            currency_code: None,
            attribute: Some("customer_id".into()),
        },
    )
    .await
    .expect("a per-customer budget");

    // Two different promotions on the same campaign: what races here is the
    // campaign's shared per-customer row, not either promotion's own counter.
    let first_promo = on_campaign(&mut setup, &ctx, "RACE-A", campaign.id).await;
    let second_promo = on_campaign(&mut setup, &ctx, "RACE-B", campaign.id).await;
    setup.commit().await.expect("to keep the setup");

    let first = async {
        let mut tx = shop.begin().await;
        let taken =
            promotion::claim(&mut tx, &shop.ctx(), first_promo, Some(customer), nothing()).await;
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
    let second = async {
        let mut tx = shop.begin().await;
        let taken = promotion::claim(
            &mut tx,
            &shop.ctx(),
            second_promo,
            Some(customer),
            nothing(),
        )
        .await;
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
    let (first, second) = tokio::join!(first, second);

    assert_eq!(
        i32::from(first.is_ok()) + i32::from(second.is_ok()),
        1,
        "a customer's last unit of a campaign budget went to both orders or to neither"
    );

    let mut after = shop.begin().await;
    let used: Decimal = sqlx::query_scalar(
        "select u.used
         from campaign_budget_usage u
         join campaign_budget b on b.id = u.campaign_budget_id and b.scope = u.scope
         where u.scope = $1 and b.campaign_id = $2",
    )
    .bind(shop.here.0)
    .bind(campaign.id.as_uuid())
    .fetch_one(&mut *after)
    .await
    .expect("the per-customer counter");
    after.commit().await.expect("to finish reading");
    assert_eq!(used, dec!(1));

    shop.close().await;
}
