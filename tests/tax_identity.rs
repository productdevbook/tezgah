//! Who the shop is, who the buyer is, and where the goods leave from.
//!
//! Every one of these was unreachable until the routes existed: with nothing
//! writing a registration, a number or a certificate, `decide` had no input
//! and answered "domestic sale to a consumer" for every sale there has ever
//! been. So these are written against the writers, and they check the
//! treatment the reading side lands on.

mod common;

use common::Shop;
use rust_decimal_macros::dec;
use tezgah::api::store as api_store;
use tezgah::money::{Currency, Money};
use tezgah::ports::{Ctx, Tx};
use tezgah::tax::{
    NewCustomerTaxId, NewTaxExemption, NewTaxRate, NewTaxRegion, NewTaxRegistration, TaxEvidence,
    TaxTreatment, TaxableAddress, TaxableLine,
};
use tezgah::{customer, inventory, store, tax};
use uuid::Uuid;

fn euro() -> Currency {
    Currency::parse("EUR").expect("a currency code")
}

async fn seed_euro(tx: &mut Tx<'_>, scope: Uuid) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'EUR', 2, '€', '€', 'Euro')",
    )
    .bind(Uuid::now_v7())
    .bind(scope)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

async fn a_rate_of(tx: &mut Tx<'_>, ctx: &Ctx<'_>, country: &str, percent: rust_decimal::Decimal) {
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
    .expect("a tax region");

    tax::create_tax_rate(
        tx,
        ctx,
        NewTaxRate {
            tax_region_id: region.id,
            rate: percent,
            code: Some("vat".into()),
            name: "VAT".into(),
            is_default: true,
            is_combinable: false,
        },
    )
    .await
    .expect("a default rate");
}

async fn registered_in(tx: &mut Tx<'_>, ctx: &Ctx<'_>, country: &str, scheme: &str, home: bool) {
    tax::register_shop(
        tx,
        ctx,
        NewTaxRegistration {
            country_code: country.into(),
            scheme: scheme.into(),
            tax_id: None,
            is_home: home,
            valid_from: None,
            valid_until: None,
        },
    )
    .await
    .expect("a registration");
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

fn one_line(amount: rust_decimal::Decimal) -> Vec<TaxableLine> {
    vec![TaxableLine {
        id: Uuid::now_v7(),
        amount: Money::new(amount, euro()),
        targets: Vec::new(),
        tax_code: None,
        address: None,
    }]
}

#[tokio::test]
async fn a_french_business_with_a_checked_number_accounts_for_its_own_vat() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;
    a_rate_of(&mut tx, &ctx, "FR", dec!(20)).await;
    registered_in(&mut tx, &ctx, "DE", "domestic", true).await;

    let buyer = a_buyer(&mut tx, &ctx).await;
    tax::record_tax_id(
        &mut tx,
        &ctx,
        NewCustomerTaxId {
            customer_id: buyer,
            tax_id: "FR12345678901".into(),
            tax_id_type: "vat".into(),
            tax_id_country: "FR".into(),
            validated_at: Some(chrono::Utc::now()),
            evidence: Some("consultation-1".into()),
        },
    )
    .await
    .expect("a checked number");

    let mut subject = tax::subject_for(&mut tx, &ctx, Some(buyer), true, Vec::new(), None)
        .await
        .expect("a subject");
    subject.is_business = true;

    let shifted = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("FR"),
        Some(&subject),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(shifted[0].treatment, TaxTreatment::ReverseCharge);
    assert_eq!(shifted[0].amount.amount, dec!(0));

    // The same sale to a business that gave no number is a sale at the French
    // rate: the shift rests on the number, not on the buyer saying it is one.
    let stranger = a_buyer(&mut tx, &ctx).await;
    let mut plain = tax::subject_for(&mut tx, &ctx, Some(stranger), true, Vec::new(), None)
        .await
        .expect("a subject");
    plain.is_business = true;

    let charged = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("FR"),
        Some(&plain),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(charged[0].treatment, TaxTreatment::Standard);
    assert_eq!(charged[0].amount.amount, dec!(20.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_french_consumer_is_placed_by_two_pieces_and_filed_under_oss() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;
    a_rate_of(&mut tx, &ctx, "FR", dec!(20)).await;
    registered_in(&mut tx, &ctx, "DE", "domestic", true).await;
    registered_in(&mut tx, &ctx, "FR", "oss", false).await;

    let placed = tax::subject_for(
        &mut tx,
        &ctx,
        None,
        false,
        vec![
            TaxEvidence::billing_address("FR"),
            TaxEvidence::shipping_address("FR"),
        ],
        None,
    )
    .await
    .expect("a subject");

    let lines = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("FR"),
        Some(&placed),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(lines[0].treatment, TaxTreatment::Oss);
    assert_eq!(
        lines[0].amount.amount,
        dec!(20.00),
        "OSS charges the buyer's own rate; it changes which return it lands on"
    );

    let one_piece = tax::subject_for(
        &mut tx,
        &ctx,
        None,
        false,
        vec![TaxEvidence::billing_address("FR")],
        None,
    )
    .await
    .expect("a subject");

    let refused = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("FR"),
        Some(&one_piece),
        false,
    )
    .await
    .expect_err("one piece of evidence places nobody");
    assert!(
        format!("{refused}").contains("two pieces of evidence"),
        "the refusal has to say what is missing rather than quietly taxing it at home"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn an_exempt_institution_pays_nothing_until_the_certificate_is_revoked() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;
    a_rate_of(&mut tx, &ctx, "DE", dec!(19)).await;
    registered_in(&mut tx, &ctx, "DE", "domestic", true).await;

    let buyer = a_buyer(&mut tx, &ctx).await;
    let granted = tax::grant_exemption(
        &mut tx,
        &ctx,
        NewTaxExemption {
            customer_id: buyer,
            kind: "government".into(),
            reason_code: Some("public_body".into()),
            certificate_reference: Some("REF-1".into()),
            country_code: "DE".into(),
            province_code: None,
            valid_from: Some(chrono::Utc::now() - chrono::Duration::days(1)),
            valid_until: None,
            verified_at: None,
            evidence: None,
        },
    )
    .await
    .expect("a certificate");

    let exempt = tax::subject_for(&mut tx, &ctx, Some(buyer), true, Vec::new(), None)
        .await
        .expect("a subject");
    let free = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("DE"),
        Some(&exempt),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(free[0].treatment, TaxTreatment::Exempt);
    assert_eq!(free[0].amount.amount, dec!(0));

    tax::revoke_exemption(
        &mut tx,
        &ctx,
        granted.id,
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
        &TaxableAddress::to("DE"),
        Some(&after),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(charged[0].treatment, TaxTreatment::Standard);
    assert_eq!(charged[0].amount.amount, dec!(19.00));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_warehouse_address_is_written_read_and_decides_the_border() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;
    a_rate_of(&mut tx, &ctx, "FR", dec!(20)).await;

    let held = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: Some(inventory::NewStockLocationAddress {
                address_1: "1 Example Way".into(),
                country_code: "de".into(),
                city: Some("Anytown".into()),
                ..inventory::NewStockLocationAddress::default()
            }),
        },
    )
    .await
    .expect("a location");
    assert!(held.address_id.is_some(), "the insert bound the address");

    let read = inventory::stock_location_address(&mut tx, &ctx, held.id)
        .await
        .expect("to read it")
        .expect("an address");
    assert_eq!(read.country_code, "DE");

    let moved = inventory::set_stock_location_address(
        &mut tx,
        &ctx,
        held.id,
        inventory::NewStockLocationAddress {
            address_1: "2 Example Way".into(),
            country_code: "DE".into(),
            ..inventory::NewStockLocationAddress::default()
        },
    )
    .await
    .expect("to move it");
    assert_eq!(moved.id, read.id, "one location, one address row");
    assert_eq!(moved.address_1, "2 Example Way");

    let origin = inventory::origin_country(&mut tx, &ctx)
        .await
        .expect("an origin");
    assert_eq!(origin.as_deref(), Some("DE"));

    // The shop has registered nowhere, so the warehouse is the only thing that
    // knows the sale left Germany.
    let buyer = a_buyer(&mut tx, &ctx).await;
    tax::record_tax_id(
        &mut tx,
        &ctx,
        NewCustomerTaxId {
            customer_id: buyer,
            tax_id: "FR12345678901".into(),
            tax_id_type: "vat".into(),
            tax_id_country: "FR".into(),
            validated_at: Some(chrono::Utc::now()),
            evidence: Some("consultation-2".into()),
        },
    )
    .await
    .expect("a checked number");

    let mut subject = tax::subject_for(&mut tx, &ctx, Some(buyer), true, Vec::new(), origin)
        .await
        .expect("a subject");
    subject.is_business = true;

    let lines = tax::calculate(
        &mut tx,
        &ctx,
        &one_line(dec!(100)),
        &TaxableAddress::to("FR"),
        Some(&subject),
        false,
    )
    .await
    .expect("a calculation");
    assert_eq!(lines[0].treatment, TaxTreatment::ReverseCharge);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_delivery_country_resolves_to_the_region_that_serves_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;

    let region = store::create_region(
        &mut tx,
        &ctx,
        store::NewRegion {
            name: format!("europe {}", Uuid::now_v7()),
            currency_code: euro(),
            is_tax_inclusive: false,
        },
    )
    .await
    .expect("a region");

    assert!(
        store::region_for_country(&mut tx, &ctx, "FR")
            .await
            .expect("an answer")
            .is_none(),
        "an empty table decides nothing, which is what every shop running today has"
    );

    store::add_region_country(
        &mut tx,
        &ctx,
        region.id,
        store::NewRegionCountry {
            iso_2: "fr".into(),
            iso_3: "fra".into(),
            numeric_code: "250".into(),
            name: "France".into(),
            display_name: None,
        },
    )
    .await
    .expect("a country");

    let found = store::region_for_country(&mut tx, &ctx, "FR")
        .await
        .expect("an answer")
        .expect("a region");
    assert_eq!(found.id, region.id);
    assert_eq!(found.currency_code, "EUR");

    let listed = store::region_countries(&mut tx, &ctx, region.id, tezgah::page::Paging::first(10))
        .await
        .expect("a page");
    assert_eq!(listed.items.len(), 1);
    assert_eq!(listed.items[0].display_name, "France");

    store::remove_region_country(&mut tx, &ctx, "FR")
        .await
        .expect("to take it out");
    assert!(
        store::region_for_country(&mut tx, &ctx, "FR")
            .await
            .expect("an answer")
            .is_none()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_tax_number_stays_out_of_what_an_error_says() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let buyer = a_buyer(&mut tx, &ctx).await;
    let refused = tax::record_tax_id(
        &mut tx,
        &ctx,
        NewCustomerTaxId {
            customer_id: buyer,
            tax_id: "FR12345678901".into(),
            tax_id_type: "vat".into(),
            tax_id_country: "FRANCE".into(),
            validated_at: None,
            evidence: None,
        },
    )
    .await
    .expect_err("that is not a country code");

    assert!(
        !format!("{refused}").contains("FR12345678901"),
        "a number in an error text is a number in every log that catches it"
    );
    assert!(!refused.report().contains("FR12345678901"));

    let certificate = tax::grant_exemption(
        &mut tx,
        &ctx,
        NewTaxExemption {
            customer_id: buyer,
            kind: "government".into(),
            reason_code: None,
            certificate_reference: Some("REF-SECRET".into()),
            country_code: "DE".into(),
            province_code: None,
            valid_from: Some(chrono::Utc::now()),
            valid_until: Some(chrono::Utc::now() - chrono::Duration::days(1)),
            verified_at: None,
            evidence: None,
        },
    )
    .await
    .expect_err("an exemption expires after it starts");
    assert!(!format!("{certificate}").contains("REF-SECRET"));
    assert!(!certificate.report().contains("REF-SECRET"));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_cart_priced_in_one_currency_is_not_delivered_to_another_region() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    seed_euro(&mut tx, shop.here.0).await;
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, '₺', '₺', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a currency");

    let region = store::create_region(
        &mut tx,
        &ctx,
        store::NewRegion {
            name: format!("europe {}", Uuid::now_v7()),
            currency_code: euro(),
            is_tax_inclusive: false,
        },
    )
    .await
    .expect("a region");

    let token = store::create_publishable_key(&mut tx, &ctx, "storefront")
        .await
        .expect("a token")
        .token;

    let in_lira = api_store::create_cart(
        &mut tx,
        &ctx,
        &token,
        api_store::CreateCart {
            currency_code: "TRY".into(),
            region_id: None,
            sales_channel_id: None,
            email: None,
        },
    )
    .await
    .expect("a cart");

    let to_france = api_store::UpdateCart {
        shipping_address: Some(api_store::AddressInput {
            address_1: Some("1 Example Way".into()),
            country_code: Some("FR".into()),
            ..api_store::AddressInput::default()
        }),
        ..api_store::UpdateCart::default()
    };

    api_store::update_cart(&mut tx, &ctx, in_lira.id, to_france.clone())
        .await
        .expect("France is served by nobody yet, so nothing contradicts anything");

    store::add_region_country(
        &mut tx,
        &ctx,
        region.id,
        store::NewRegionCountry {
            iso_2: "FR".into(),
            iso_3: "FRA".into(),
            numeric_code: "250".into(),
            name: "France".into(),
            display_name: None,
        },
    )
    .await
    .expect("a country");

    let clash = api_store::update_cart(&mut tx, &ctx, in_lira.id, to_france.clone())
        .await
        .expect_err("France is served in euro and this cart is in lira");
    assert!(format!("{clash}").contains("another currency"));

    let in_euro = api_store::create_cart(
        &mut tx,
        &ctx,
        &token,
        api_store::CreateCart {
            currency_code: "EUR".into(),
            region_id: Some(region.id),
            sales_channel_id: None,
            email: None,
        },
    )
    .await
    .expect("a cart");

    api_store::update_cart(&mut tx, &ctx, in_euro.id, to_france)
        .await
        .expect("the region's own currency");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
