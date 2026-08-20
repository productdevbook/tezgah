mod common;

use common::Shop;
use tezgah::customer::{self, CustomerFilter, CustomerPatch, NewAddress, NewCustomer};
use tezgah::page::{Paging, Search};
use tezgah::payment;

#[tokio::test]
async fn a_guest_and_an_account_are_the_same_table() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let guest = customer::create(&mut tx, &ctx, NewCustomer::guest()).await?;
    let member = customer::create(&mut tx, &ctx, NewCustomer::account("Ada@example.com")).await?;

    assert!(!guest.has_account);
    assert!(member.has_account);
    assert_eq!(member.email.as_deref(), Some("ada@example.com"));

    let listed =
        customer::list(&mut tx, &ctx, CustomerFilter::default(), Paging::first(10)).await?;
    assert_eq!(listed.len(), 2);
    assert!(shop.host.audited("customer"));

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_account_without_an_email_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let new = NewCustomer {
        has_account: true,
        ..NewCustomer::default()
    };
    assert!(customer::create(&mut tx, &ctx, new).await.is_err());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_new_default_address_stands_the_old_one_down() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("bo@example.com")).await?;

    let first = customer::add_address(
        &mut tx,
        &ctx,
        who.id,
        NewAddress {
            address_1: Some("1 First Street".into()),
            country_code: Some("tr".into()),
            is_default_shipping: true,
            is_default_billing: true,
            ..NewAddress::default()
        },
    )
    .await?;
    assert_eq!(first.country_code.as_deref(), Some("TR"));

    let second = customer::add_address(
        &mut tx,
        &ctx,
        who.id,
        NewAddress {
            address_1: Some("2 Second Street".into()),
            is_default_shipping: true,
            ..NewAddress::default()
        },
    )
    .await?;

    let addresses = customer::addresses(&mut tx, &ctx, who.id, Paging::first(10)).await?;
    let defaults: Vec<_> = addresses
        .items
        .iter()
        .filter(|address| address.is_default_shipping)
        .map(|address| address.id)
        .collect();
    assert_eq!(defaults, vec![second.id]);

    // Billing was not claimed by the second address, so it stayed where it was.
    let billing: Vec<_> = addresses
        .items
        .iter()
        .filter(|address| address.is_default_billing)
        .map(|address| address.id)
        .collect();
    assert_eq!(billing, vec![first.id]);

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_country_code_that_is_not_two_letters_is_refused() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("cy@example.com")).await?;
    let bad = customer::add_address(
        &mut tx,
        &ctx,
        who.id,
        NewAddress {
            country_code: Some("TUR".into()),
            ..NewAddress::default()
        },
    )
    .await;
    assert!(bad.is_err());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn groups_hold_members_and_let_them_go() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("dee@example.com")).await?;
    let group = customer::create_group(&mut tx, &ctx, "  Wholesale  ", None).await?;
    assert_eq!(group.name, "Wholesale");

    customer::join_group(&mut tx, &ctx, who.id, group.id).await?;
    // Joining twice is the same membership, not a second one.
    customer::join_group(&mut tx, &ctx, who.id, group.id).await?;

    assert_eq!(
        customer::group_ids(&mut tx, &ctx, who.id).await?,
        vec![group.id]
    );
    let members = customer::members(&mut tx, &ctx, group.id, Paging::first(10)).await?;
    assert_eq!(members.len(), 1);

    customer::leave_group(&mut tx, &ctx, who.id, group.id).await?;
    assert!(customer::group_ids(&mut tx, &ctx, who.id).await?.is_empty());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn erasing_empties_the_person_and_keeps_the_row() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("eve@example.com")).await?;
    customer::update(
        &mut tx,
        &ctx,
        who.id,
        CustomerPatch {
            phone: Some("+90 555 000 00 00".into()),
            first_name: Some("Eve".into()),
            ..CustomerPatch::default()
        },
    )
    .await?;
    customer::add_address(
        &mut tx,
        &ctx,
        who.id,
        NewAddress {
            address_1: Some("3 Third Street".into()),
            ..NewAddress::default()
        },
    )
    .await?;

    let erased = customer::erase(&mut tx, &ctx, who.id).await?;
    assert_eq!(erased.id, who.id);
    assert!(erased.email.is_none());
    assert!(erased.phone.is_none());
    assert!(erased.is_anonymised());

    let still_there = customer::get(&mut tx, &ctx, who.id).await?;
    assert!(still_there.is_anonymised());
    assert!(shop.host.emitted("customer.anonymised"));

    let left = customer::addresses(&mut tx, &ctx, who.id, Paging::first(10)).await?;
    assert!(left.is_empty());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn an_export_carries_the_addresses_and_the_groups() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("fay@example.com")).await?;
    customer::add_address(
        &mut tx,
        &ctx,
        who.id,
        NewAddress {
            address_1: Some("4 Fourth Street".into()),
            ..NewAddress::default()
        },
    )
    .await?;
    let group = customer::create_group(&mut tx, &ctx, "Loyal", None).await?;
    customer::join_group(&mut tx, &ctx, who.id, group.id).await?;

    let document = customer::export(&mut tx, &ctx, who.id).await?;
    assert_eq!(
        document["customer"]["email"].as_str(),
        Some("fay@example.com")
    );
    assert_eq!(document["addresses"].as_array().map(Vec::len), Some(1));
    assert_eq!(document["groups"].as_array().map(Vec::len), Some(1));
    assert_eq!(document["carts"].as_array().map(Vec::len), Some(0));
    assert_eq!(document["orders"].as_array().map(Vec::len), Some(0));
    // The shop's own identifier is not the customer's data.
    assert!(document["customer"].get("scope").is_none());

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn another_scope_sees_none_of_it() -> tezgah::Result<()> {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let who = customer::create(
        &mut mine,
        &shop.ctx(),
        NewCustomer::account("gil@example.com"),
    )
    .await?;
    mine.commit().await?;

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let ctx = shop.theirs();
    assert!(customer::get(&mut theirs, &ctx, who.id).await.is_err());
    assert!(
        customer::list(
            &mut theirs,
            &ctx,
            CustomerFilter::default(),
            Paging::first(10)
        )
        .await?
        .is_empty()
    );
    assert!(customer::export(&mut theirs, &ctx, who.id).await.is_err());
    theirs.rollback().await.ok();

    shop.close().await;
    Ok(())
}

/// #194: `erase` used to leave `account_holder` — a real email and the
/// provider's own reference to a saved card — sitting under a customer it had
/// just anonymised everywhere else.
#[tokio::test]
async fn erase_scrubs_the_account_holder_the_customer_saved() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let who = customer::create(&mut tx, &ctx, NewCustomer::account("erase-me@example.com")).await?;

    payment::register_provider(&mut tx, &ctx, "mock").await?;
    let holder = payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: "mock".into(),
            customer_id: Some(who.id),
            external_id: "cus_erase_me".into(),
            email: Some("erase-me@example.com".into()),
            data: serde_json::json!({ "brand": "visa" }),
        },
    )
    .await?;

    customer::erase(&mut tx, &ctx, who.id).await?;

    assert!(
        payment::account_holder_by_id(&mut tx, &ctx, holder.id)
            .await?
            .is_none(),
        "the account holder still answers to its own lookup after erase"
    );

    let (email, external_id, data): (Option<String>, String, serde_json::Value) = sqlx::query_as(
        "select email, external_id, data from account_holder where scope = $1 and id = $2",
    )
    .bind(shop.here.0)
    .bind(holder.id.as_uuid())
    .fetch_one(&mut *tx)
    .await?;
    assert!(email.is_none(), "the real email survived erase");
    assert_ne!(
        external_id, "cus_erase_me",
        "the provider's own reference survived erase"
    );
    assert_eq!(
        data,
        serde_json::json!({}),
        "the provider's stored data survived erase"
    );

    tx.rollback().await.ok();
    shop.close().await;
    Ok(())
}

/// The four ways somebody asks for a person, and the one that is not here.
#[tokio::test]
async fn a_customer_is_found_by_the_four_things_they_are_called() -> tezgah::Result<()> {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let ada = customer::create(
        &mut tx,
        &ctx,
        NewCustomer {
            email: Some("ada@example.com".into()),
            first_name: Some("Ada".into()),
            last_name: Some("Lovelace".into()),
            company_name: Some("Analytical Engines".into()),
            ..NewCustomer::default()
        },
    )
    .await?;

    customer::create(
        &mut tx,
        &ctx,
        NewCustomer {
            email: Some("grace@example.com".into()),
            ..NewCustomer::default()
        },
    )
    .await?;

    let searching = |text: &str| CustomerFilter {
        search: Search::new(text),
        ..CustomerFilter::default()
    };

    for wanted in ["ADA@", "ada", "lovelace", "analytical"] {
        let found = customer::list(&mut tx, &ctx, searching(wanted), Paging::first(10)).await?;
        assert_eq!(found.len(), 1, "{wanted} finds exactly Ada");
        assert_eq!(found.items[0].id, ada.id);
    }

    let nobody = customer::list(&mut tx, &ctx, searching("nobody"), Paging::first(10)).await?;
    assert!(nobody.is_empty());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
    Ok(())
}
