//! The shop's own channels and the keys a storefront reaches them with,
//! against a real Postgres.

mod common;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{Doorman, Shop};
use tezgah::id::{PublishableKeyId, SalesChannelId};
use tezgah::page::Paging;
use tezgah::ports::{
    Action, Actor, AuditEntry, AuditSink, Authorizer, Clock, Event, EventSink, JobSpec, Jobs,
    Permit, Resource, Tx,
};
use tezgah::store::{self, NewSalesChannel, SalesChannel, SalesChannelPatch, StorePatch};

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
