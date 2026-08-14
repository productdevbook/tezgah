//! Tax, against a real Postgres.
//!
//! The two questions that matter are whether an inclusive price and an
//! exclusive one both land on the same tax at the line, and whether a rate
//! configured in one shop can be seen from another.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::money::{Currency, Money};
use tezgah::ports::{Ctx, Tx};
use tezgah::tax::{
    self, NewTaxRate, NewTaxRateRule, NewTaxRegion, TaxReference, TaxTarget, TaxableAddress,
    TaxableLine,
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

async fn a_flat_eighteen(tx: &mut Tx<'_>, ctx: &Ctx<'_>) {
    let region = tax::create_tax_region(
        tx,
        ctx,
        NewTaxRegion {
            country_code: "TR".into(),
            province_code: None,
            parent_id: None,
            provider: None,
        },
    )
    .await
    .expect("a country");

    tax::create_tax_rate(
        tx,
        ctx,
        NewTaxRate {
            tax_region_id: region.id,
            rate: dec!(18),
            code: Some("vat".into()),
            name: "VAT".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("a default rate");
}

fn one_line(amount: Decimal) -> Vec<TaxableLine> {
    vec![TaxableLine {
        id: Uuid::now_v7(),
        amount: Money::new(amount, lira()),
        targets: Vec::new(),
    }]
}

fn to_turkey() -> TaxableAddress {
    TaxableAddress {
        country_code: "TR".into(),
        province_code: None,
    }
}

#[tokio::test]
async fn tax_is_added_to_a_price_that_excludes_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;

    let lines = tax::calculate(&mut tx, &ctx, &one_line(dec!(100)), &to_turkey(), false)
        .await
        .expect("a calculation");

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].amount.amount, dec!(18.00));
    assert!(!lines[0].is_tax_inclusive);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn tax_is_taken_out_of_a_price_that_includes_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;

    let lines = tax::calculate(&mut tx, &ctx, &one_line(dec!(118)), &to_turkey(), true)
        .await
        .expect("a calculation");

    assert_eq!(lines.len(), 1);
    // Out of the price, not on top of it: 118 * 18% would be 21.24.
    assert_eq!(lines[0].amount.amount, dec!(18.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_combinable_province_rate_sits_on_top_of_the_country_rate() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;

    let country = tax::create_tax_region(
        &mut tx,
        &ctx,
        NewTaxRegion {
            country_code: "TR".into(),
            province_code: None,
            parent_id: None,
            provider: None,
        },
    )
    .await
    .expect("a country");

    tax::create_tax_rate(
        &mut tx,
        &ctx,
        NewTaxRate {
            tax_region_id: country.id,
            rate: dec!(18),
            code: Some("vat".into()),
            name: "VAT".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("a country rate");

    let province = tax::create_tax_region(
        &mut tx,
        &ctx,
        NewTaxRegion {
            country_code: "TR".into(),
            province_code: Some("06".into()),
            parent_id: Some(country.id),
            provider: None,
        },
    )
    .await
    .expect("a province");

    let local = tax::create_tax_rate(
        &mut tx,
        &ctx,
        NewTaxRate {
            tax_region_id: province.id,
            rate: dec!(2),
            code: Some("local".into()),
            name: "City levy".into(),
            is_default: false,
            is_combinable: true,
        },
    )
    .await
    .expect("a local rate");

    let product = Uuid::now_v7();
    tax::create_tax_rate_rule(
        &mut tx,
        &ctx,
        NewTaxRateRule {
            tax_rate_id: local.id,
            reference: TaxReference::Product,
            reference_id: product,
        },
    )
    .await
    .expect("a rule");

    let lines = vec![TaxableLine {
        id: Uuid::now_v7(),
        amount: Money::new(dec!(100), lira()),
        targets: vec![TaxTarget {
            reference: TaxReference::Product,
            id: product,
        }],
    }];

    let address = TaxableAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
    };

    let out = tax::calculate(&mut tx, &ctx, &lines, &address, false)
        .await
        .expect("a calculation");

    assert_eq!(
        out.len(),
        2,
        "a combinable rate stacks rather than replaces"
    );
    let total: Decimal = out.iter().map(|line| line.amount.amount).sum();
    assert_eq!(total, dec!(20.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn one_shops_rates_are_invisible_to_another() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    seed_currency(&mut mine, shop.here.0).await;
    a_flat_eighteen(&mut mine, &shop.ctx()).await;
    mine.commit().await.expect("to keep the rates");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let seen = tax::tax_regions(&mut theirs, &shop.theirs(), tezgah::Paging::first(10))
        .await
        .expect("a page");
    assert!(seen.is_empty());

    let lines = tax::calculate(
        &mut theirs,
        &shop.theirs(),
        &one_line(dec!(100)),
        &to_turkey(),
        false,
    )
    .await
    .expect("a calculation");
    assert!(lines.is_empty(), "another shop's rates were charged");

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn two_currencies_in_one_calculation_are_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;

    let mixed = vec![
        TaxableLine {
            id: Uuid::now_v7(),
            amount: Money::new(dec!(100), lira()),
            targets: Vec::new(),
        },
        TaxableLine {
            id: Uuid::now_v7(),
            amount: Money::new(dec!(100), Currency::parse("USD").expect("a currency code")),
            targets: Vec::new(),
        },
    ];

    let refused = tax::calculate(&mut tx, &ctx, &mixed, &to_turkey(), false)
        .await
        .expect_err("two currencies taxed as one");
    assert!(refused.is_internal(), "a mixed calculation was not a bug");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
