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
use tezgah::{cart, catalogue, inventory};
use uuid::Uuid;

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
        tax_code: None,
        address: None,
    }]
}

fn to_turkey() -> TaxableAddress {
    TaxableAddress::to("TR")
}

#[tokio::test]
async fn tax_is_added_to_a_price_that_excludes_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;

    let lines = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &to_turkey(),
        None,
        false,
    )
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

    let lines = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(118)),
        &to_turkey(),
        None,
        true,
    )
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
        tax_code: None,
        address: None,
    }];

    let address = TaxableAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
        postal_code: None,
    };

    let out = tax::calculate(&mut tx, &ctx, &lines, &address, None, false)
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

/// A variant-specific rate — a reduced rate on one magazine subscription,
/// say — has to reach the line even though the address's own rate is the
/// shop's default. Before #127, `TaxableLine` carried no variant target, so
/// a rule narrowed to a variant could never match and the address rate won
/// by default.
#[tokio::test]
async fn a_variant_specific_rate_reaches_the_line_over_the_address_rate() {
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
            code: Some("standard".into()),
            name: "Standard".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("the address's own default rate");

    let reduced = tax::create_tax_rate(
        &mut tx,
        &ctx,
        NewTaxRate {
            tax_region_id: country.id,
            rate: dec!(1),
            code: Some("reduced".into()),
            name: "Reduced".into(),
            is_default: false,
            is_combinable: false,
        },
    )
    .await
    .expect("a reduced rate");

    let variant = Uuid::now_v7();
    tax::create_tax_rate_rule(
        &mut tx,
        &ctx,
        NewTaxRateRule {
            tax_rate_id: reduced.id,
            reference: TaxReference::Variant,
            reference_id: variant,
        },
    )
    .await
    .expect("a variant rule");

    let lines = vec![TaxableLine {
        id: Uuid::now_v7(),
        amount: Money::new(dec!(100), lira()),
        targets: vec![TaxTarget {
            reference: TaxReference::Variant,
            id: variant,
        }],
        tax_code: None,
        address: None,
    }];

    let address = TaxableAddress {
        country_code: "TR".into(),
        province_code: None,
        postal_code: None,
    };

    let out = tax::calculate(&mut tx, &ctx, &lines, &address, None, false)
        .await
        .expect("a calculation");

    assert_eq!(out.len(), 1, "the variant's own rate, not the address's");
    assert_eq!(out[0].rate, dec!(1));
    assert_eq!(out[0].amount.amount, dec!(1.00));

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
        None,
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
            tax_code: None,
            address: None,
        },
        TaxableLine {
            id: Uuid::now_v7(),
            amount: Money::new(dec!(100), Currency::parse("USD").expect("a currency code")),
            targets: Vec::new(),
            tax_code: None,
            address: None,
        },
    ];

    let refused = tax::calculate(&mut tx, &ctx, &mixed, &to_turkey(), None, false)
        .await
        .expect_err("two currencies taxed as one");
    assert!(refused.is_internal(), "a mixed calculation was not a bug");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

// ---------------------------------------------------------------------------
// Who the buyer is, and the paper the answer rests on.
// ---------------------------------------------------------------------------

use async_trait::async_trait;
use tezgah::customer;
use tezgah::tax::{
    NewCustomerTaxId, NewTaxExemption, NewTaxRegistration, TaxEvidence, TaxJurisdiction, TaxLine,
    TaxProvider, TaxQuote, TaxTreatment,
};

async fn a_shop_at_home_in(tx: &mut Tx<'_>, ctx: &Ctx<'_>, country: &str) {
    tax::register_shop(
        tx,
        ctx,
        NewTaxRegistration {
            country_code: country.into(),
            scheme: "domestic".into(),
            tax_id: None,
            is_home: true,
            valid_from: None,
            valid_until: None,
        },
    )
    .await
    .expect("a home registration");
}

async fn a_buyer(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> tezgah::id::CustomerId {
    customer::create(
        tx,
        ctx,
        customer::NewCustomer {
            email: Some(format!("{}@example.com", Uuid::now_v7().simple())),
            ..customer::NewCustomer::default()
        },
    )
    .await
    .expect("a customer")
    .id
}

#[tokio::test]
async fn a_checked_vat_number_across_a_border_moves_the_charge_to_the_buyer() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_shop_at_home_in(&mut tx, &ctx, "TR").await;

    let buyer = a_buyer(&mut tx, &ctx).await;
    tax::record_tax_id(
        &mut tx,
        &ctx,
        NewCustomerTaxId {
            customer_id: buyer,
            tax_id: "DE811234567".into(),
            tax_id_type: "vat".into(),
            tax_id_country: "DE".into(),
            validated_at: Some(chrono::Utc::now()),
            evidence: Some("WAPIAAAAX0000000".into()),
        },
    )
    .await
    .expect("a checked number");

    let mut subject = tax::subject_for(
        &mut tx,
        &ctx,
        Some(buyer),
        true,
        vec![TaxEvidence {
            source: "billing_address".into(),
            country_code: "DE".into(),
        }],
        None,
    )
    .await
    .expect("a subject");
    assert_eq!(subject.tax_ids.len(), 1);
    subject.is_business = true;

    let lines = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("DE"),
        Some(&subject),
        false,
    )
    .await
    .expect("a calculation");

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].amount.amount, Decimal::ZERO);
    assert_eq!(lines[0].treatment, TaxTreatment::ReverseCharge);
    assert_eq!(lines[0].tax_id.as_deref(), Some("DE811234567"));
    assert_eq!(
        lines[0].tax_id_evidence.as_deref(),
        Some("WAPIAAAAX0000000"),
        "the consultation number is the proof, and it has to be kept"
    );
    assert_eq!(lines[0].evidence.len(), 1);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_certificate_exempts_a_buyer_until_it_runs_out() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;
    a_shop_at_home_in(&mut tx, &ctx, "TR").await;

    let buyer = a_buyer(&mut tx, &ctx).await;
    let live = tax::grant_exemption(
        &mut tx,
        &ctx,
        NewTaxExemption {
            customer_id: buyer,
            kind: "resale_certificate".into(),
            reason_code: Some("resale".into()),
            certificate_reference: Some("ST-120".into()),
            country_code: "TR".into(),
            province_code: None,
            valid_from: Some(chrono::Utc::now() - chrono::Duration::days(1)),
            valid_until: Some(chrono::Utc::now() + chrono::Duration::days(30)),
            verified_at: None,
            evidence: None,
        },
    )
    .await
    .expect("a certificate");

    let subject = tax::subject_for(&mut tx, &ctx, Some(buyer), true, Vec::new(), None)
        .await
        .expect("a subject");

    let exempt = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &to_turkey(),
        Some(&subject),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(exempt[0].treatment, TaxTreatment::Exempt);
    assert_eq!(exempt[0].amount.amount, Decimal::ZERO);
    assert_eq!(exempt[0].exemption_id, Some(live.id));

    // The same buyer, the same certificate, a day after it expired.
    tax::revoke_exemption(
        &mut tx,
        &ctx,
        live.id,
        Some(chrono::Utc::now() - chrono::Duration::hours(1)),
    )
    .await
    .expect("to end it");

    let after = tax::subject_for(&mut tx, &ctx, Some(buyer), true, Vec::new(), None)
        .await
        .expect("a subject");
    let charged = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &to_turkey(),
        Some(&after),
        false,
    )
    .await
    .expect("a calculation");

    assert_eq!(charged[0].treatment, TaxTreatment::Standard);
    assert_eq!(
        charged[0].amount.amount,
        dec!(18.00),
        "an expired certificate exempts nothing"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn another_shop_cannot_see_a_buyers_numbers_or_certificates() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let ctx = shop.ctx();
    let buyer = a_buyer(&mut mine, &ctx).await;
    tax::record_tax_id(
        &mut mine,
        &ctx,
        NewCustomerTaxId {
            customer_id: buyer,
            tax_id: "DE811234567".into(),
            tax_id_type: "vat".into(),
            tax_id_country: "DE".into(),
            validated_at: Some(chrono::Utc::now()),
            evidence: Some("WAPIAAAAX0000000".into()),
        },
    )
    .await
    .expect("a number");
    a_shop_at_home_in(&mut mine, &ctx, "TR").await;
    mine.commit().await.expect("to keep it");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let seen = tax::tax_ids(&mut theirs, &shop.theirs(), buyer)
        .await
        .expect("a read");
    assert!(seen.is_empty(), "another shop read a buyer's VAT number");

    let certificates = tax::exemptions(&mut theirs, &shop.theirs(), buyer)
        .await
        .expect("a read");
    assert!(certificates.is_empty());

    let registrations = tax::registrations(&mut theirs, &shop.theirs())
        .await
        .expect("a read");
    assert!(
        registrations.is_empty(),
        "another shop read where this one is registered"
    );

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A provider that answers the way a United States one does: one line per
/// authority, and a transaction id it will want back on a refund.
struct Streets;

#[async_trait]
impl TaxProvider for Streets {
    fn code(&self) -> &'static str {
        "streets"
    }

    async fn tax_lines(
        &self,
        lines: &[tax::TaxableLine],
        address: &TaxableAddress,
        _subject: Option<&tax::TaxSubject>,
        is_tax_inclusive: bool,
    ) -> tezgah::Result<TaxQuote> {
        let mut out = Vec::new();
        for line in lines {
            for (level, code, name, rate) in [
                ("state", "us-ny-state", "New York State", dec!(4.0)),
                ("county", "us-ny-nassau", "Nassau County", dec!(4.25)),
                (
                    "special",
                    "us-ny-mctd",
                    "Metropolitan district",
                    dec!(0.375),
                ),
            ] {
                out.push(TaxLine {
                    rate,
                    code: code.into(),
                    name: name.into(),
                    amount: Money::new(
                        (line.amount.amount * rate / dec!(100)).round_dp(2),
                        line.amount.currency,
                    ),
                    is_tax_inclusive,
                    jurisdiction: Some(TaxJurisdiction {
                        level: level.into(),
                        code: code.into(),
                        name: Some(name.into()),
                    }),
                    ..TaxLine::nil(
                        line.id,
                        line.amount.currency,
                        TaxTreatment::Standard,
                        chrono::Utc::now(),
                    )
                });
            }
        }

        let _ = address;
        Ok(TaxQuote {
            transaction_id: Some("streets-txn-1".into()),
            lines: out,
        })
    }

    async fn refund(
        &self,
        original_transaction_id: &str,
        lines: &[tax::TaxableLine],
        _address: &TaxableAddress,
    ) -> tezgah::Result<TaxQuote> {
        Ok(TaxQuote {
            transaction_id: Some(format!("credit-of-{original_transaction_id}")),
            lines: lines
                .iter()
                .map(|line| {
                    TaxLine::nil(
                        line.id,
                        line.amount.currency,
                        TaxTreatment::Standard,
                        chrono::Utc::now(),
                    )
                })
                .collect(),
        })
    }
}

#[tokio::test]
async fn a_us_style_answer_stays_one_line_per_authority_and_still_adds_up() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let lines = one_line(dec!(100));
    let out = tax::calculate_with(
        &ctx,
        &Streets,
        &lines,
        &TaxableAddress {
            country_code: "US".into(),
            province_code: Some("NY".into()),
            postal_code: Some("11501".into()),
        },
        None,
        false,
    )
    .await
    .expect("a quote");

    assert_eq!(out.len(), 3, "each authority is remitted separately");
    let total: Decimal = out.iter().map(|line| line.amount.amount).sum();
    assert_eq!(total, dec!(8.63), "4.00 + 4.25 + 0.375 of a hundred");

    let levels: Vec<&str> = out
        .iter()
        .filter_map(|line| line.jurisdiction.as_ref().map(|j| j.level.as_str()))
        .collect();
    assert_eq!(levels, ["state", "county", "special"]);

    for line in &out {
        assert_eq!(line.provider.as_deref(), Some("streets"));
        assert_eq!(
            line.provider_transaction_id.as_deref(),
            Some("streets-txn-1"),
            "a refund has to name the document it credits"
        );
        assert_eq!(line.address.country_code, "US");
    }

    shop.close().await;
}

#[tokio::test]
async fn a_refund_names_the_transaction_it_reverses() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let credited = tax::refund_with(
        &ctx,
        &Streets,
        "streets-txn-1",
        &one_line(dec!(100)),
        &TaxableAddress::to("US"),
    )
    .await
    .expect("a credit");

    assert_eq!(
        credited[0].provider_transaction_id.as_deref(),
        Some("credit-of-streets-txn-1")
    );

    let refused = tax::refund_with(
        &ctx,
        &Streets,
        "  ",
        &one_line(dec!(100)),
        &TaxableAddress::to("US"),
    )
    .await
    .expect_err("a credit with nothing to credit");
    assert!(!refused.is_internal(), "an empty reference is the caller's");

    shop.close().await;
}

#[tokio::test]
async fn a_taxed_cart_keeps_what_it_was_taxed_with_when_the_rate_moves() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_flat_eighteen(&mut tx, &ctx).await;

    let cart_id = seed_cart(&mut tx, shop.here.0).await;
    let line_id = seed_line(&mut tx, shop.here.0, cart_id).await;

    let taxed = tax::calculate(
        &mut tx,
        &ctx,
        &[TaxableLine {
            id: line_id,
            amount: Money::new(dec!(100), lira()),
            targets: Vec::new(),
            tax_code: Some("txcd_99999999".into()),
            address: None,
        }],
        &to_turkey(),
        None,
        false,
    )
    .await
    .expect("a calculation");

    tax::set_cart_tax_lines(
        &mut tx,
        &ctx,
        tezgah::id::CartId::from_uuid(cart_id),
        &taxed,
        &[],
    )
    .await
    .expect("to write the snapshot");

    // The shop puts the rate up afterwards, as shops do.
    let rates = tax::tax_rates(&mut tx, &ctx, None, tezgah::Paging::first(10))
        .await
        .expect("the rates");
    tax::update_tax_rate(
        &mut tx,
        &ctx,
        rates.items[0].id,
        tezgah::tax::TaxRatePatch {
            rate: Some(dec!(25)),
            ..Default::default()
        },
    )
    .await
    .expect("a new rate");

    let (rate, treatment, code, country): (Decimal, String, Option<String>, Option<String>) =
        sqlx::query_as(
            "select rate, treatment, tax_code, address_country_code
             from cart_line_item_tax_line
             where scope = $1 and cart_line_item_id = $2",
        )
        .bind(shop.here.0)
        .bind(line_id)
        .fetch_one(&mut *tx)
        .await
        .expect("the snapshot");

    assert_eq!(rate, dec!(18.00000), "the snapshot moved with the table");
    assert_eq!(treatment, "standard");
    assert_eq!(code.as_deref(), Some("txcd_99999999"));
    assert_eq!(country.as_deref(), Some("TR"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_jurisdiction_breakdown_lands_as_several_rows_under_one_line() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    let cart_id = seed_cart(&mut tx, shop.here.0).await;
    let line_id = seed_line(&mut tx, shop.here.0, cart_id).await;

    let quoted = tax::calculate_with(
        &ctx,
        &Streets,
        &[TaxableLine {
            id: line_id,
            amount: Money::new(dec!(100), lira()),
            targets: Vec::new(),
            tax_code: None,
            address: None,
        }],
        &TaxableAddress {
            country_code: "US".into(),
            province_code: Some("NY".into()),
            postal_code: Some("11501".into()),
        },
        None,
        false,
    )
    .await
    .expect("a quote");

    tax::set_cart_tax_lines(
        &mut tx,
        &ctx,
        tezgah::id::CartId::from_uuid(cart_id),
        &quoted,
        &[],
    )
    .await
    .expect("to write three rows");

    let stored: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "select code, jurisdiction_level, provider_transaction_id
         from cart_line_item_tax_line
         where scope = $1 and cart_line_item_id = $2
         order by jurisdiction_code",
    )
    .bind(shop.here.0)
    .bind(line_id)
    .fetch_all(&mut *tx)
    .await
    .expect("the rows");

    assert_eq!(
        stored.len(),
        3,
        "one row per authority, not one row per line"
    );
    assert!(
        stored
            .iter()
            .all(|(_, _, txn)| txn.as_deref() == Some("streets-txn-1"))
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

async fn seed_cart(tx: &mut Tx<'_>, scope: uuid::Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query("insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')")
        .bind(id)
        .bind(scope)
        .execute(&mut **tx)
        .await
        .expect("a cart");
    id
}

async fn seed_line(tx: &mut Tx<'_>, scope: uuid::Uuid, cart_id: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    sqlx::query(
        "insert into cart_line_item
             (id, scope, cart_id, product_title, quantity, unit_price, currency_code)
         values ($1, $2, $3, 'A thing', 1, 100, 'TRY')",
    )
    .bind(id)
    .bind(scope)
    .bind(cart_id)
    .execute(&mut **tx)
    .await
    .expect("a line");
    id
}

// ---------------------------------------------------------------------------
// One cart, two places of supply
// ---------------------------------------------------------------------------

async fn a_country_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, country: &str, percent: Decimal) {
    let region = tax::create_tax_region(
        tx,
        ctx,
        NewTaxRegion {
            country_code: country.into(),
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
            rate: percent,
            code: Some("vat".into()),
            name: format!("{country} VAT"),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("a default rate");
}

async fn a_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    handle: &str,
    ships: Option<bool>,
) -> tezgah::id::VariantId {
    let product = catalogue::create_product(
        tx,
        ctx,
        catalogue::NewProduct {
            handle: handle.into(),
            title: format!("A {handle}"),
            ..catalogue::NewProduct::default()
        },
    )
    .await
    .expect("a product");

    let variant = catalogue::create_variant(
        tx,
        ctx,
        product.id,
        catalogue::NewVariant {
            title: "One size".into(),
            sku: Some(format!("{handle}-1")),
            ..catalogue::NewVariant::default()
        },
    )
    .await
    .expect("a variant");

    match ships {
        Some(requires_shipping) => {
            let item = inventory::create_inventory_item(
                tx,
                ctx,
                inventory::NewInventoryItem {
                    sku: Some(format!("{handle}-stock")),
                    title: None,
                    requires_shipping,
                },
            )
            .await
            .expect("an inventory item");

            inventory::attach_inventory_item(tx, ctx, variant.id, item.id, 1)
                .await
                .expect("the variant to consume the item");
        }
        // Not tracked is not the same fact as digital: say so on the
        // catalogue itself rather than leaving it to an absent inventory
        // link, which now defaults to physical.
        None => {
            catalogue::update_variant(
                tx,
                ctx,
                variant.id,
                catalogue::VariantPatch {
                    requires_shipping: Some(false),
                    ..catalogue::VariantPatch::default()
                },
            )
            .await
            .expect("to mark the variant non-physical");
        }
    }

    variant.id
}

#[tokio::test]
async fn a_book_and_an_audiobook_are_taxed_in_two_different_countries() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_currency(&mut tx, shop.here.0).await;
    a_country_rate(&mut tx, &ctx, "DE", dec!(19)).await;
    a_country_rate(&mut tx, &ctx, "FR", dec!(20)).await;

    let book = a_variant(&mut tx, &ctx, "book", Some(true)).await;
    let audiobook = a_variant(&mut tx, &ctx, "audiobook", None).await;

    let cart = cart::create(
        &mut tx,
        &ctx,
        cart::NewCart::guest(Currency::parse("TRY").expect("a currency")),
    )
    .await
    .expect("a cart");

    // The parcel goes to Germany; the buyer is in France, which is where an
    // electronic service is supplied.
    cart::set_addresses(
        &mut tx,
        &ctx,
        cart.id,
        Some(cart::CartAddress {
            country_code: Some("DE".into()),
            ..cart::CartAddress::default()
        }),
        Some(cart::CartAddress {
            country_code: Some("FR".into()),
            ..cart::CartAddress::default()
        }),
    )
    .await
    .expect("two addresses");

    for (variant, price) in [(book, dec!(100)), (audiobook, dec!(200))] {
        cart::add_line(
            &mut tx,
            &ctx,
            cart.id,
            cart::AddLine {
                variant_id: variant,
                quantity: 1,
                unit_price: Money::new(price, Currency::parse("TRY").expect("a currency")),
                is_tax_inclusive: false,
            },
        )
        .await
        .expect("a line");
    }

    tezgah::api::store::reprice(&mut tx, &ctx, cart.id)
        .await
        .expect("the cart to be worked out again");

    #[derive(sqlx::FromRow)]
    struct Row {
        variant_id: Option<Uuid>,
        country: Option<String>,
        rate: Decimal,
        calculated_at: chrono::DateTime<chrono::Utc>,
    }

    let rows = sqlx::query_as::<_, Row>(
        "select l.variant_id, t.address_country_code as country, t.rate, t.calculated_at
         from cart_line_item_tax_line t
         join cart_line_item l on l.scope = t.scope and l.id = t.cart_line_item_id
         where t.scope = $1 and l.cart_id = $2",
    )
    .bind(shop.here.0)
    .bind(cart.id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the tax lines");

    assert_eq!(rows.len(), 2, "one tax line each");

    for row in &rows {
        let (country, rate) = if row.variant_id == Some(book.as_uuid()) {
            ("DE", dec!(19))
        } else {
            ("FR", dec!(20))
        };
        assert_eq!(row.country.as_deref(), Some(country));
        assert_eq!(row.rate, rate);
    }

    assert_eq!(
        rows[0].calculated_at, rows[1].calculated_at,
        "the two lines came out of two calculations rather than one"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
