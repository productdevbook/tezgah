//! The shop's own channels and the keys a storefront reaches them with,
//! against a real Postgres.

mod common;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{Doorman, Shop};
use rust_decimal::Decimal;
use tezgah::cart::{self, NewCart};
use tezgah::id::{PublishableKeyId, SalesChannelId};
use tezgah::money::Currency;
use tezgah::page::Paging;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Ctx, Event, EventSink, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use tezgah::store::{self, NewSalesChannel, NewStore, SalesChannel, SalesChannelPatch, StorePatch};

async fn a_channel(shop: &Shop, tx: &mut Tx<'_>, name: &str) -> SalesChannel {
    store::create_sales_channel(
        tx,
        &shop.ctx(),
        NewSalesChannel {
            name: name.into(),
            description: None,
            is_disabled: false,
        },
    )
    .await
    .expect("a channel")
}

/// A shop's own settings row, the way a host's onboarding creates one.
async fn a_store_row(tx: &mut Tx<'_>, ctx: &Ctx<'_>) {
    store::create_store(
        tx,
        ctx,
        NewStore {
            name: "Test shop".into(),
            default_currency_code: Currency::parse("TRY").expect("a currency"),
            supported_currency_codes: Vec::new(),
            supported_locales: Vec::new(),
            default_region_id: None,
            default_sales_channel_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a store row");
}

/// #190: `store` had no writer at all — `update store set ...` matched no
/// row and answered `Ok` while changing nothing. `create_store` gives the
/// table its first row, and `update_store` now has one to change.
#[tokio::test]
async fn a_store_row_is_created_and_update_store_actually_changes_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let created = store::create_store(
        &mut tx,
        &ctx,
        NewStore {
            name: "Original name".into(),
            default_currency_code: Currency::parse("TRY").expect("a currency"),
            supported_currency_codes: Vec::new(),
            supported_locales: Vec::new(),
            default_region_id: None,
            default_sales_channel_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a shop's first settings row");
    assert_eq!(created.name, "Original name");
    assert_eq!(created.default_currency_code, "TRY");
    assert_eq!(created.supported_currency_codes, vec!["TRY".to_string()]);

    let updated = store::update_store(
        &mut tx,
        &ctx,
        StorePatch {
            name: Some("Renamed shop".into()),
            ..StorePatch::default()
        },
    )
    .await
    .expect("updating the row that now exists");
    assert_eq!(
        updated.name, "Renamed shop",
        "update_store changed the row it just read, not nothing"
    );

    let read_back = store::store(&mut tx, &ctx)
        .await
        .expect("the row persisted");
    assert_eq!(read_back.name, "Renamed shop");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// #190: `store_scope_key` says one row per scope; `create_store` inserts
/// with `on conflict (scope) do nothing`, so a second call for the same scope
/// finds nothing to return and answers a conflict rather than quietly
/// keeping the first shop's settings.
#[tokio::test]
async fn a_second_store_row_for_the_same_scope_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    a_store_row(&mut tx, &ctx).await;

    let second = store::create_store(
        &mut tx,
        &ctx,
        NewStore {
            name: "A second shop".into(),
            default_currency_code: Currency::parse("USD").expect("a currency"),
            supported_currency_codes: Vec::new(),
            supported_locales: Vec::new(),
            default_region_id: None,
            default_sales_channel_id: None,
            metadata: None,
        },
    )
    .await
    .expect_err("a scope only ever gets one store row");
    assert!(second.is_conflict());

    let still_there = store::store(&mut tx, &ctx).await.expect("the first row");
    assert_eq!(still_there.default_currency_code, "TRY");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_channel_is_made_edited_and_taken_away_again() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let channel = a_channel(&shop, &mut tx, "Web").await;
    assert!(!channel.is_disabled);

    let edited = store::update_sales_channel(
        &mut tx,
        &ctx,
        channel.id,
        SalesChannelPatch {
            name: Some("Web shop".into()),
            is_disabled: Some(true),
            ..SalesChannelPatch::default()
        },
    )
    .await
    .expect("the edit");
    assert_eq!(edited.name, "Web shop");
    assert!(edited.is_disabled);

    let listed = store::sales_channels(&mut tx, &ctx, Paging::first(10))
        .await
        .expect("the channels");
    assert_eq!(listed.items.len(), 1);

    store::delete_sales_channel(&mut tx, &ctx, channel.id)
        .await
        .expect("the delete");
    let gone = store::sales_channel(&mut tx, &ctx, channel.id)
        .await
        .expect_err("it was deleted");
    assert!(gone.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_key_is_readable_once_and_only_its_hash_is_kept() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let issued = store::create_publishable_key(&mut tx, &ctx, "Storefront")
        .await
        .expect("a key");
    assert!(issued.token.starts_with("pk_"));

    let held: String = sqlx::query_scalar("select token from publishable_key where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the stored token");
    assert_ne!(held, issued.token, "the token itself was stored");
    assert_eq!(held.len(), 64, "what is stored is not a sha256 digest");

    let listed = store::publishable_keys(&mut tx, &ctx, Paging::first(10))
        .await
        .expect("the keys");
    assert_eq!(listed.items.len(), 1);
    assert!(listed.items[0].is_live());

    let refused = store::channels_for_token(&mut tx, &ctx, "pk_not_the_one")
        .await
        .expect_err("a token nobody issued");
    assert!(refused.is_denied());

    store::channels_for_token(&mut tx, &ctx, &issued.token)
        .await
        .expect("the issued token");

    let revoked = store::revoke_publishable_key(&mut tx, &ctx, issued.key.id)
        .await
        .expect("the revoke");
    assert!(!revoked.is_live());

    let refused = store::channels_for_token(&mut tx, &ctx, &issued.token)
        .await
        .expect_err("a revoked key");
    assert!(refused.is_denied());

    let again = store::revoke_publishable_key(&mut tx, &ctx, issued.key.id)
        .await
        .expect_err("it was already revoked");
    assert!(again.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_key_sees_the_channels_it_is_linked_to_and_no_others() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let web = a_channel(&shop, &mut tx, "Web").await;
    let counter = a_channel(&shop, &mut tx, "Counter").await;

    let issued = store::create_publishable_key(&mut tx, &ctx, "Storefront")
        .await
        .expect("a key");

    store::link_key_to_channel(&mut tx, &ctx, issued.key.id, web.id)
        .await
        .expect("the link");

    let seen = store::channels_for_token(&mut tx, &ctx, &issued.token)
        .await
        .expect("what the storefront may see");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].id, web.id);
    assert!(
        !seen.iter().any(|channel| channel.id == counter.id),
        "a key saw a channel it was never given"
    );

    store::unlink_key_from_channel(&mut tx, &ctx, issued.key.id, web.id)
        .await
        .expect("the unlink");
    assert!(
        store::channels_for_key(&mut tx, &ctx, issued.key.id)
            .await
            .expect("its channels")
            .is_empty()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn one_shops_channels_and_keys_are_invisible_to_another() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let channel = a_channel(&shop, &mut mine, "Web").await;
    let issued = store::create_publishable_key(&mut mine, &shop.ctx(), "Storefront")
        .await
        .expect("a key");
    store::link_key_to_channel(&mut mine, &shop.ctx(), issued.key.id, channel.id)
        .await
        .expect("the link");
    mine.commit().await.expect("to keep them");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let ctx = shop.theirs();

    let listed = store::sales_channels(&mut theirs, &ctx, Paging::first(10))
        .await
        .expect("their channels");
    assert!(
        listed.items.is_empty(),
        "another shop's channels were listed"
    );

    let listed = store::publishable_keys(&mut theirs, &ctx, Paging::first(10))
        .await
        .expect("their keys");
    assert!(listed.items.is_empty(), "another shop's keys were listed");

    let refused = store::channels_for_token(&mut theirs, &ctx, &issued.token)
        .await
        .expect_err("somebody else's token");
    assert!(refused.is_denied());

    let refused = store::channels_for_key(&mut theirs, &ctx, issued.key.id)
        .await
        .expect("no rows rather than somebody else's");
    assert!(refused.is_empty());

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A host granting tezgah's pricing power and nothing else — the obvious thing
/// to give whoever edits prices.
#[derive(Debug, Default)]
struct PricesOnly;

impl Authorizer for PricesOnly {
    fn authorize(&self, _: &Actor, _: Action, resource: &Resource) -> tezgah::Result<Permit> {
        match resource {
            Resource::Pricing => Ok(Permit::granted()),
            _ => Err(tezgah::Error::denied()),
        }
    }
}

impl Clock for PricesOnly {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[async_trait]
impl AuditSink for PricesOnly {
    async fn record(&self, _: &mut Tx<'_>, _: AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl EventSink for PricesOnly {
    async fn emit(&self, _: &mut Tx<'_>, _: Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Jobs for PricesOnly {
    async fn enqueue(&self, _: &mut Tx<'_>, _: JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

/// Minting a storefront credential is not editing a price, and a host must be
/// able to grant the one without the other.
#[tokio::test]
async fn a_key_is_not_reachable_with_permission_to_edit_prices() {
    let shop = Shop::open().await;
    let host = PricesOnly;
    let ctx = shop.ctx_as(
        Actor::Staff {
            id: uuid::Uuid::now_v7(),
        },
        &host,
    );
    let mut tx = shop.begin().await;

    let key = PublishableKeyId::new();
    let channel = SalesChannelId::new();

    assert!(
        store::create_publishable_key(&mut tx, &ctx, "Storefront")
            .await
            .expect_err("a price editor does not mint credentials")
            .is_denied()
    );
    assert!(
        store::publishable_keys(&mut tx, &ctx, Paging::first(10))
            .await
            .expect_err("nor read them")
            .is_denied()
    );
    assert!(
        store::publishable_key(&mut tx, &ctx, key)
            .await
            .expect_err("nor read one")
            .is_denied()
    );
    assert!(
        store::revoke_publishable_key(&mut tx, &ctx, key)
            .await
            .expect_err("nor revoke one")
            .is_denied()
    );
    assert!(
        store::link_key_to_channel(&mut tx, &ctx, key, channel)
            .await
            .expect_err("nor point one at a channel")
            .is_denied()
    );
    assert!(
        store::unlink_key_from_channel(&mut tx, &ctx, key, channel)
            .await
            .expect_err("nor take it away again")
            .is_denied()
    );
    assert!(
        store::channels_for_token(&mut tx, &ctx, "pk_whatever")
            .await
            .expect_err("nor spend a token")
            .is_denied()
    );

    assert!(
        store::update_store(&mut tx, &ctx, StorePatch::default())
            .await
            .expect_err("nor rewrite the shop's own settings")
            .is_denied()
    );
    assert!(
        store::create_sales_channel(
            &mut tx,
            &ctx,
            NewSalesChannel {
                name: "Web".into(),
                description: None,
                is_disabled: false,
            },
        )
        .await
        .expect_err("nor open a channel to sell through")
        .is_denied()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// The other half: that the calls ask at all rather than reading the rows.
#[tokio::test]
async fn a_host_that_refuses_everything_is_obeyed_by_the_key_calls() {
    let shop = Shop::open().await;
    let doorman = Doorman;
    let ctx = shop.ctx_as(
        Actor::Staff {
            id: uuid::Uuid::now_v7(),
        },
        &doorman,
    );
    let mut tx = shop.begin().await;

    let key = PublishableKeyId::new();

    assert!(
        store::create_publishable_key(&mut tx, &ctx, "Storefront")
            .await
            .expect_err("nothing is minted")
            .is_denied()
    );
    assert!(
        store::publishable_keys(&mut tx, &ctx, Paging::first(10))
            .await
            .expect_err("nothing is listed")
            .is_denied()
    );
    assert!(
        store::revoke_publishable_key(&mut tx, &ctx, key)
            .await
            .expect_err("nothing is revoked")
            .is_denied()
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A refusal that came from catching a constraint violation leaves the caller
/// holding an aborted transaction, so the assertion that matters is the query
/// that comes after it.
#[tokio::test]
async fn a_second_channel_of_the_same_name_is_refused_without_killing_the_transaction() {
    let shop = Shop::open().await;
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    a_channel(&shop, &mut tx, "Web").await;

    let refused = store::create_sales_channel(
        &mut tx,
        &ctx,
        NewSalesChannel {
            name: "Web".into(),
            description: None,
            is_disabled: false,
        },
    )
    .await
    .expect_err("one channel of that name");
    assert!(refused.is_conflict());

    let second = a_channel(&shop, &mut tx, "Shop").await;
    assert_eq!(second.name, "Shop");

    let counted: i64 = sqlx::query_scalar("select count(*) from sales_channel where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the transaction to still be usable");
    assert_eq!(counted, 2);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// #180: `currency` has no writer besides `store::create_currency`, and a
/// currency it enables is what every rounding call site reads back.
#[tokio::test]
async fn a_currency_is_enabled_and_then_usable_in_a_cart() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let euro = Currency::parse("EUR").expect("a currency code");

    let opened = cart::create(&mut tx, &ctx, NewCart::guest(euro))
        .await
        .expect("a cart in a currency this shop has not enabled yet");

    let refused = cart::totals(&mut tx, &ctx, opened.id)
        .await
        .expect_err("EUR has no row in currency yet");
    assert!(refused.is_not_found());

    let made = store::create_currency(
        &mut tx,
        &ctx,
        store::NewCurrency {
            code: euro,
            numeric_code: Some("978".into()),
            exponent: 2,
            symbol: "€".into(),
            symbol_native: "€".into(),
            name: "Euro".into(),
        },
    )
    .await
    .expect("enabling a currency");
    assert_eq!(made.code, "EUR");
    assert_eq!(made.exponent, 2);

    let listed = store::currencies(&mut tx, &ctx)
        .await
        .expect("the currencies");
    assert!(listed.iter().any(|row| row.code == "EUR"));

    let totals = cart::totals(&mut tx, &ctx, opened.id)
        .await
        .expect("EUR now has a row to round with");
    assert_eq!(totals.total.amount, Decimal::ZERO);

    // Enabling an already-enabled currency updates it rather than colliding.
    let reenabled = store::create_currency(
        &mut tx,
        &ctx,
        store::NewCurrency {
            code: euro,
            numeric_code: None,
            exponent: 2,
            symbol: "€".into(),
            symbol_native: "eur".into(),
            name: "Euro".into(),
        },
    )
    .await
    .expect("enabling the same currency again");
    assert_eq!(reenabled.id, made.id, "the same row, not a duplicate");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// #183: a store's default sales channel cannot be deleted out from under it.
#[tokio::test]
async fn the_default_sales_channel_cannot_be_deleted() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    a_store_row(&mut tx, &ctx).await;
    let channel = a_channel(&shop, &mut tx, "Web").await;

    store::update_store(
        &mut tx,
        &ctx,
        StorePatch {
            default_sales_channel_id: Some(channel.id),
            ..StorePatch::default()
        },
    )
    .await
    .expect("setting the default");

    let refused = store::delete_sales_channel(&mut tx, &ctx, channel.id)
        .await
        .expect_err("it is the shop's default");
    assert!(refused.is_conflict());

    let other = a_channel(&shop, &mut tx, "Other").await;
    store::update_store(
        &mut tx,
        &ctx,
        StorePatch {
            default_sales_channel_id: Some(other.id),
            ..StorePatch::default()
        },
    )
    .await
    .expect("changing the default");

    store::delete_sales_channel(&mut tx, &ctx, channel.id)
        .await
        .expect("no longer the default, so deletable");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
