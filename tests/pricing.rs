//! What a thing costs, asked of a real Postgres.
//!
//! Every branch of resolution gets a test of its own: the exact match, the
//! candidate satisfying the most rules, the priority tie-break, the ruleless
//! default, the quantity band, and the sale list that must not move the amount
//! shown struck through.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::money::{Currency, Money};
use tezgah::page::Paging;
use tezgah::ports::Ctx;
use tezgah::ports::Tx;
use tezgah::pricing::{
    self, CalculatedPrice, NewPrice, NewPriceList, NewPriceRule, PriceContext, PriceListUpdate,
    PriceUpdate,
};
use uuid::Uuid;

fn try_() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: Decimal) -> Money {
    Money::new(amount, try_())
}

fn base(set: tezgah::id::PriceSetId, amount: Decimal) -> NewPrice {
    NewPrice {
        price_set_id: set,
        price_list_id: None,
        title: None,
        amount: money(amount),
        min_quantity: None,
        max_quantity: None,
        rules: Vec::new(),
    }
}

async fn resolve(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    set: tezgah::id::PriceSetId,
    at: &PriceContext,
) -> CalculatedPrice {
    pricing::resolve(tx, ctx, set, at)
        .await
        .expect("resolution to run")
        .expect("a price to resolve")
}

#[tokio::test]
async fn a_price_set_with_one_ruleless_price_answers_with_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("a price");

    let found = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(found.calculated.amount, dec!(100));
    assert_eq!(found.original.amount, dec!(100));
    assert!(found.price_list_id.is_none());
    assert!(shop.host.audited("price"));

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_price_matching_the_whole_context_beats_one_matching_part_of_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let region = tezgah::id::RegionId::new();
    let group = Uuid::now_v7();

    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the default");

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![NewPriceRule::eq("region_id", region.to_string())],
            ..base(set.id, dec!(90))
        },
    )
    .await
    .expect("the regional price");

    let exact = pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![
                NewPriceRule::eq("currency_code", "TRY"),
                NewPriceRule::eq("region_id", region.to_string()),
                NewPriceRule::eq("customer_group_id", group.to_string()),
            ],
            ..base(set.id, dec!(80))
        },
    )
    .await
    .expect("the exact price");

    let at = PriceContext::new(try_(), 1)
        .in_region(region)
        .for_group(group);
    let found = resolve(&mut tx, &ctx, set.id, &at).await;

    assert_eq!(found.calculated.amount, dec!(80));
    assert_eq!(found.price_id, exact.id);

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn the_candidate_satisfying_the_most_rules_wins_when_none_is_exact() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let region = tezgah::id::RegionId::new();
    let group = Uuid::now_v7();
    let channel = Uuid::now_v7();

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![NewPriceRule::eq("region_id", region.to_string())],
            ..base(set.id, dec!(90))
        },
    )
    .await
    .expect("one rule");

    let two = pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![
                NewPriceRule::eq("region_id", region.to_string()),
                NewPriceRule::eq("customer_group_id", group.to_string()),
            ],
            ..base(set.id, dec!(85))
        },
    )
    .await
    .expect("two rules");

    // Four context pairs, so no candidate is an exact match.
    let at = PriceContext::new(try_(), 1)
        .in_region(region)
        .for_group(group)
        .through(channel);
    let found = resolve(&mut tx, &ctx, set.id, &at).await;

    assert_eq!(found.price_id, two.id);
    assert_eq!(found.calculated.amount, dec!(85));

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn priority_settles_two_candidates_carrying_the_same_number_of_rules() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let region = tezgah::id::RegionId::new();

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![NewPriceRule::eq("region_id", region.to_string())],
            ..base(set.id, dec!(70))
        },
    )
    .await
    .expect("the low priority price");

    let loud = pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![NewPriceRule::eq("region_id", region.to_string()).with_priority(10)],
            ..base(set.id, dec!(95))
        },
    )
    .await
    .expect("the high priority price");

    let at = PriceContext::new(try_(), 1)
        .in_region(region)
        .through(Uuid::now_v7());
    let found = resolve(&mut tx, &ctx, set.id, &at).await;

    assert_eq!(
        found.price_id, loud.id,
        "priority did not settle the tie, the cheaper amount did"
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_context_no_rule_matches_falls_back_to_the_ruleless_default() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let default = pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the default");
    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            rules: vec![NewPriceRule::eq(
                "region_id",
                tezgah::id::RegionId::new().to_string(),
            )],
            ..base(set.id, dec!(50))
        },
    )
    .await
    .expect("a price for somewhere else");

    let at = PriceContext::new(try_(), 1).in_region(tezgah::id::RegionId::new());
    let found = resolve(&mut tx, &ctx, set.id, &at).await;

    assert_eq!(found.price_id, default.id);
    assert_eq!(found.calculated.amount, dec!(100));

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_quantity_outside_the_band_does_not_get_the_bands_price() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the default");
    let bulk = pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            min_quantity: Some(10),
            max_quantity: Some(20),
            ..base(set.id, dec!(80))
        },
    )
    .await
    .expect("the bulk price");

    let one = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(one.calculated.amount, dec!(100));

    let ten = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 10)).await;
    assert_eq!(ten.price_id, bulk.id);

    let twenty = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 20)).await;
    assert_eq!(twenty.price_id, bulk.id, "the band includes its own edges");

    let many = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 21)).await;
    assert_eq!(many.calculated.amount, dec!(100));

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_sale_list_moves_the_amount_charged_and_leaves_the_one_struck_through() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let full = pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the shelf price");

    let list = pricing::create_price_list(
        &mut tx,
        &ctx,
        NewPriceList {
            title: "Spring".into(),
            description: None,
            kind: "sale".into(),
            status: "active".into(),
            starts_at: None,
            ends_at: None,
        },
    )
    .await
    .expect("a price list");

    let reduced = pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_list_id: Some(list.id),
            ..base(set.id, dec!(60))
        },
    )
    .await
    .expect("the sale price");

    let found = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;

    assert_eq!(found.price_id, reduced.id);
    assert_eq!(found.calculated.amount, dec!(60));
    assert_eq!(found.original.amount, dec!(100));
    assert_eq!(found.original_price_id, full.id);
    assert_eq!(found.price_list_id, Some(list.id));
    assert!(found.is_reduced());

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn an_override_list_moves_the_amount_struck_through_as_well() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the shelf price");

    let list = pricing::create_price_list(
        &mut tx,
        &ctx,
        NewPriceList {
            title: "Wholesale".into(),
            description: None,
            kind: "override".into(),
            status: "active".into(),
            starts_at: None,
            ends_at: None,
        },
    )
    .await
    .expect("a price list");

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_list_id: Some(list.id),
            ..base(set.id, dec!(60))
        },
    )
    .await
    .expect("the override price");

    let found = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(found.calculated.amount, dec!(60));
    assert_eq!(found.original.amount, dec!(60));
    assert!(!found.is_reduced());

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_list_that_is_draft_or_out_of_its_window_prices_nothing() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the shelf price");

    let list = pricing::create_price_list(
        &mut tx,
        &ctx,
        NewPriceList {
            title: "Later".into(),
            description: None,
            kind: "sale".into(),
            status: "draft".into(),
            starts_at: None,
            ends_at: None,
        },
    )
    .await
    .expect("a price list");

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_list_id: Some(list.id),
            ..base(set.id, dec!(10))
        },
    )
    .await
    .expect("the sale price");

    let found = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(found.calculated.amount, dec!(100), "a draft list priced");

    pricing::update_price_list(
        &mut tx,
        &ctx,
        list.id,
        PriceListUpdate {
            status: Some("active".into()),
            starts_at: Some(Some(ctx.now() + chrono::Duration::days(1))),
            ..PriceListUpdate::default()
        },
    )
    .await
    .expect("to open the window later");

    let found = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(
        found.calculated.amount,
        dec!(100),
        "a list that has not started yet priced"
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_list_rule_keeps_the_list_off_a_context_it_does_not_cover() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the shelf price");

    let list = pricing::create_price_list(
        &mut tx,
        &ctx,
        NewPriceList {
            title: "Members".into(),
            description: None,
            kind: "sale".into(),
            status: "active".into(),
            starts_at: None,
            ends_at: None,
        },
    )
    .await
    .expect("a price list");

    let members = Uuid::now_v7();
    let rule = pricing::add_price_list_rule(
        &mut tx,
        &ctx,
        list.id,
        "customer_group_id",
        vec![members.to_string()],
    )
    .await
    .expect("a list rule");
    assert_eq!(rule.allowed_values, vec![members.to_string()]);

    pricing::add_price(
        &mut tx,
        &ctx,
        NewPrice {
            price_list_id: Some(list.id),
            ..base(set.id, dec!(70))
        },
    )
    .await
    .expect("the members' price");

    let stranger = resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1)).await;
    assert_eq!(stranger.calculated.amount, dec!(100));

    let member = resolve(
        &mut tx,
        &ctx,
        set.id,
        &PriceContext::new(try_(), 1).for_group(members),
    )
    .await;
    assert_eq!(member.calculated.amount, dec!(70));

    let counted = pricing::price_list(&mut tx, &ctx, list.id)
        .await
        .expect("the list back");
    assert_eq!(counted.rules_count, 1);

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_rule_added_after_the_price_is_counted_on_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("the default");

    let special = pricing::add_price(&mut tx, &ctx, base(set.id, dec!(75)))
        .await
        .expect("a second price");
    assert_eq!(special.rules_count, 0);

    let region = tezgah::id::RegionId::new();
    let rule = pricing::add_price_rule(
        &mut tx,
        &ctx,
        special.id,
        NewPriceRule::eq("region_id", region.to_string()),
    )
    .await
    .expect("a rule");

    let listed = pricing::prices(&mut tx, &ctx, set.id, Paging::first(10))
        .await
        .expect("the prices back");
    let counted = listed
        .items
        .iter()
        .find(|price| price.id == special.id)
        .expect("the price it was added to");
    assert_eq!(
        counted.rules_count, 1,
        "rules_count did not follow the rule that was added"
    );

    let found = resolve(
        &mut tx,
        &ctx,
        set.id,
        &PriceContext::new(try_(), 1).in_region(region),
    )
    .await;
    assert_eq!(found.price_id, special.id);

    pricing::remove_price_rule(&mut tx, &ctx, special.id, rule.id)
        .await
        .expect("to take the rule off again");

    let found = resolve(
        &mut tx,
        &ctx,
        set.id,
        &PriceContext::new(try_(), 1).in_region(region),
    )
    .await;
    assert_eq!(
        found.calculated.amount,
        dec!(75),
        "both prices are ruleless now, and the cheaper one settles the tie"
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn another_scope_cannot_see_a_price_set_or_its_prices() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let ctx = shop.ctx();
    let set = pricing::create_price_set(&mut mine, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut mine, &ctx, base(set.id, dec!(100)))
        .await
        .expect("a price");
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let elsewhere = shop.theirs();

    let refused = pricing::price_set(&mut theirs, &elsewhere, set.id).await;
    assert!(
        refused.is_err(),
        "another scope could read somebody else's price set"
    );

    let nothing = pricing::resolve(
        &mut theirs,
        &elsewhere,
        set.id,
        &PriceContext::new(try_(), 1),
    )
    .await
    .expect("resolution to run");
    assert!(
        nothing.is_none(),
        "another scope could resolve somebody else's price"
    );

    let listed = pricing::prices(&mut theirs, &elsewhere, set.id, Paging::first(10))
        .await
        .expect("to list");
    assert!(listed.is_empty());

    theirs.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_price_in_another_currency_is_not_an_answer() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("a lira price");

    let euro = Currency::parse("EUR").expect("a currency code");
    let nothing = pricing::resolve(&mut tx, &ctx, set.id, &PriceContext::new(euro, 1))
        .await
        .expect("resolution to run");
    assert!(nothing.is_none(), "a lira price answered a euro question");

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_price_set_can_be_reached_from_the_variant_it_prices() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, title, handle) values ($1, $2, 'A', 'a')")
        .bind(product)
        .bind(shop.here.0)
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = tezgah::id::VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, 'One')",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .execute(&mut *tx)
    .await
    .expect("a variant");

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    pricing::link_variant(&mut tx, &ctx, variant, set.id)
        .await
        .expect("to link it");

    let found = pricing::price_set_for_variant(&mut tx, &ctx, variant)
        .await
        .expect("to look it up");
    assert_eq!(found, Some(set.id));

    let second = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a second price set");
    pricing::link_variant(&mut tx, &ctx, variant, second.id)
        .await
        .expect("to relink it");

    let found = pricing::price_set_for_variant(&mut tx, &ctx, variant)
        .await
        .expect("to look it up");
    assert_eq!(found, Some(second.id), "a relink left two rows behind");

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_preference_says_whether_a_context_is_priced_with_tax_in_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let at = PriceContext::new(try_(), 1);
    assert!(
        !pricing::is_tax_inclusive(&mut tx, &ctx, &at)
            .await
            .expect("to read the preferences"),
        "no preference should mean tax is added on top"
    );

    pricing::set_price_preference(&mut tx, &ctx, "currency_code", Some("TRY".into()), true)
        .await
        .expect("a preference");

    assert!(
        pricing::is_tax_inclusive(&mut tx, &ctx, &at)
            .await
            .expect("to read the preferences")
    );

    let region = tezgah::id::RegionId::new();
    pricing::set_price_preference(&mut tx, &ctx, "region_id", Some(region.to_string()), false)
        .await
        .expect("a region preference");

    assert!(
        !pricing::is_tax_inclusive(&mut tx, &ctx, &at.clone().in_region(region))
            .await
            .expect("to read the preferences"),
        "the region's answer did not beat the currency's"
    );

    let read = pricing::price_preference(&mut tx, &ctx, "currency_code", Some("TRY"))
        .await
        .expect("to read one")
        .expect("the one that was written");
    assert!(read.is_tax_inclusive);

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn an_amount_can_be_changed_and_a_price_taken_off_the_shelf() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    let price = pricing::add_price(&mut tx, &ctx, base(set.id, dec!(100)))
        .await
        .expect("a price");

    let raised = pricing::update_price(
        &mut tx,
        &ctx,
        price.id,
        PriceUpdate {
            amount: Some(money(dec!(120))),
            ..PriceUpdate::default()
        },
    )
    .await
    .expect("to raise it");
    assert_eq!(raised.amount, dec!(120));

    pricing::delete_price(&mut tx, &ctx, price.id)
        .await
        .expect("to delete it");

    let nothing = pricing::resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 1))
        .await
        .expect("resolution to run");
    assert!(nothing.is_none(), "a deleted price still answered");

    assert!(
        pricing::delete_price(&mut tx, &ctx, price.id)
            .await
            .is_err(),
        "deleting it twice succeeded"
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn a_price_list_is_refused_the_things_the_column_forbids() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let bad = NewPriceList {
        title: "  ".into(),
        description: None,
        kind: "sale".into(),
        status: "draft".into(),
        starts_at: None,
        ends_at: None,
    };
    assert!(
        pricing::create_price_list(&mut tx, &ctx, bad)
            .await
            .is_err()
    );

    let bad = NewPriceList {
        title: "A".into(),
        description: None,
        kind: "clearance".into(),
        status: "draft".into(),
        starts_at: None,
        ends_at: None,
    };
    assert!(
        pricing::create_price_list(&mut tx, &ctx, bad)
            .await
            .is_err()
    );

    let now = ctx.now();
    let bad = NewPriceList {
        title: "A".into(),
        description: None,
        kind: "sale".into(),
        status: "draft".into(),
        starts_at: Some(now),
        ends_at: Some(now - chrono::Duration::days(1)),
    };
    assert!(
        pricing::create_price_list(&mut tx, &ctx, bad)
            .await
            .is_err()
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}

#[tokio::test]
async fn resolution_refuses_a_quantity_of_nothing() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let set = pricing::create_price_set(&mut tx, &ctx)
        .await
        .expect("a price set");
    assert!(
        pricing::resolve(&mut tx, &ctx, set.id, &PriceContext::new(try_(), 0))
            .await
            .is_err()
    );

    tx.commit().await.expect("to commit");
    shop.close().await;
}
