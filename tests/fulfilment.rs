//! Fulfilment, against a real Postgres.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use common::Shop;
use rust_decimal_macros::dec;
use tezgah::fulfilment::{
    self, DeliveryAddress, FulfillmentProvider, NewFulfillment, NewFulfillmentItem,
    NewFulfillmentSet, NewGeoZone, NewLabel, NewServiceZone, NewShippingOption, PriceKind, SetKind,
    Shipment, ShipmentRequest, Shippable, ShippingOptionTranslation, ZoneKind,
};
use tezgah::id::{OrderId, OrderItemId, StockLocationId};
use tezgah::money::{Currency, Money};
use tezgah::ports::{Ctx, Tx};
use uuid::Uuid;

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

/// A carrier that answers, and counts what it was asked.
#[derive(Debug, Default)]
struct Carrier {
    quotes: AtomicU32,
    shipments: AtomicU32,
    cancels: AtomicU32,
}

impl Carrier {
    fn count(counter: &AtomicU32) -> u32 {
        counter.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl FulfillmentProvider for Carrier {
    fn code(&self) -> &'static str {
        "carrier"
    }

    async fn price(&self, _request: &ShipmentRequest) -> tezgah::Result<Option<Money>> {
        self.quotes.fetch_add(1, Ordering::Relaxed);
        Ok(Some(Money::new(dec!(42), lira())))
    }

    async fn create_shipment(&self, _request: &ShipmentRequest) -> tezgah::Result<Shipment> {
        self.shipments.fetch_add(1, Ordering::Relaxed);
        Ok(Shipment {
            labels: vec![NewLabel {
                tracking_number: "CARRIER-1".into(),
                tracking_url: Some("https://example.test/CARRIER-1".into()),
                label_url: None,
            }],
            data: None,
        })
    }

    async fn cancel_shipment(&self, _request: &ShipmentRequest) -> tezgah::Result<()> {
        self.cancels.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// A zone covering one province, with one flat option on it.
async fn ankara_only(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> tezgah::id::ServiceZoneId {
    let set = fulfilment::create_set(
        tx,
        ctx,
        NewFulfillmentSet {
            name: "Delivery".into(),
            kind: SetKind::Shipping,
        },
    )
    .await
    .expect("a set");

    let zone = fulfilment::create_service_zone(
        tx,
        ctx,
        NewServiceZone {
            name: "Central".into(),
            fulfillment_set_id: set.id,
        },
    )
    .await
    .expect("a service zone");

    fulfilment::create_geo_zone(
        tx,
        ctx,
        NewGeoZone {
            kind: ZoneKind::Province,
            country_code: "TR".into(),
            province_code: Some("06".into()),
            city: None,
            postal_expression: None,
            service_zone_id: zone.id,
        },
    )
    .await
    .expect("a geo zone");

    fulfilment::create_shipping_option(
        tx,
        ctx,
        NewShippingOption {
            name: "Next day".into(),
            price_type: PriceKind::Flat,
            service_zone_id: zone.id,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("an option");

    zone.id
}

fn parcel() -> Vec<Shippable> {
    vec![Shippable {
        id: Uuid::now_v7(),
        quantity: 1,
        amount: Money::new(dec!(100), lira()),
        shipping_profile_id: None,
        requires_shipping: true,
    }]
}

/// An order with one item, two of it, and somewhere to ship it from.
async fn an_order(tx: &mut Tx<'_>, scope: uuid::Uuid) -> (OrderId, OrderItemId, StockLocationId) {
    let location = StockLocationId::new();
    sqlx::query("insert into stock_location (id, scope, name) values ($1, $2, 'Depot')")
        .bind(location.as_uuid())
        .bind(scope)
        .execute(&mut **tx)
        .await
        .expect("a location");

    let order = OrderId::new();
    sqlx::query(r#"insert into "order" (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(order.as_uuid())
        .bind(scope)
        .execute(&mut **tx)
        .await
        .expect("an order");

    let line = Uuid::now_v7();
    sqlx::query(
        "insert into order_line_item (id, scope, order_id, title, unit_price, currency_code)
         values ($1, $2, $3, 'A kettle', 100, 'TRY')",
    )
    .bind(line)
    .bind(scope)
    .bind(order.as_uuid())
    .execute(&mut **tx)
    .await
    .expect("a line item");

    let item = OrderItemId::new();
    sqlx::query(
        "insert into order_item
             (id, scope, order_id, order_line_item_id, currency_code, quantity, unit_price)
         values ($1, $2, $3, $4, 'TRY', 2, 100)",
    )
    .bind(item.as_uuid())
    .bind(scope)
    .bind(order.as_uuid())
    .bind(line)
    .execute(&mut **tx)
    .await
    .expect("an order item");

    (order, item, location)
}

fn half_of(item: OrderItemId) -> NewFulfillmentItem {
    NewFulfillmentItem {
        order_item_id: item,
        inventory_item_id: None,
        title: "A kettle".into(),
        sku: None,
        barcode: None,
        quantity: 1,
    }
}

#[tokio::test]
async fn an_address_is_offered_the_options_of_the_zone_it_falls_in() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    ankara_only(&mut tx, &ctx).await;

    let here = DeliveryAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
        ..DeliveryAddress::default()
    };
    let offered = fulfilment::options_for(&mut tx, &ctx, &here, None, &parcel())
        .await
        .expect("options");
    assert_eq!(offered.len(), 1);

    let elsewhere = DeliveryAddress {
        country_code: "TR".into(),
        province_code: Some("34".into()),
        ..DeliveryAddress::default()
    };
    let offered = fulfilment::options_for(&mut tx, &ctx, &elsewhere, None, &parcel())
        .await
        .expect("options");
    assert!(
        offered.is_empty(),
        "a zone answered for a province it does not cover"
    );

    let abroad = DeliveryAddress {
        country_code: "DE".into(),
        province_code: Some("06".into()),
        ..DeliveryAddress::default()
    };
    let offered = fulfilment::options_for(&mut tx, &ctx, &abroad, None, &parcel())
        .await
        .expect("options");
    assert!(offered.is_empty());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_fulfilment_that_has_shipped_cannot_be_cancelled() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let (order, item, location) = an_order(&mut tx, shop.here.0).await;

    let made = fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        order,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: Some("a test".into()),
            address: None,
            data: None,
            items: vec![half_of(item)],
        },
    )
    .await
    .expect("a fulfilment");

    fulfilment::add_label(
        &mut tx,
        &ctx,
        order,
        made.id,
        NewLabel {
            tracking_number: "TRK-1".into(),
            tracking_url: None,
            label_url: None,
        },
    )
    .await
    .expect("a label");

    fulfilment::mark_packed(&mut tx, &ctx, order, made.id)
        .await
        .expect("packing");
    let shipped = fulfilment::mark_shipped(&mut tx, &ctx, order, made.id, Some("a test"))
        .await
        .expect("shipping");
    assert!(shipped.shipped_at.is_some());

    let refused = fulfilment::cancel_fulfillment(&mut tx, &ctx, order, made.id)
        .await
        .expect_err("a shipped parcel cannot be recalled");
    assert!(refused.is_conflict());

    fulfilment::mark_delivered(&mut tx, &ctx, order, made.id)
        .await
        .expect("delivery");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_partial_fulfilment_leaves_the_rest_of_the_item_to_send() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let (order, item, location) = an_order(&mut tx, shop.here.0).await;

    let new = |quantity: i32| NewFulfillment {
        location_id: location,
        shipping_option_id: None,
        provider_id: None,
        requires_shipping: true,
        created_by: None,
        address: None,
        data: None,
        items: vec![NewFulfillmentItem {
            quantity,
            ..half_of(item)
        }],
    };

    fulfilment::create_fulfillment(&mut tx, &ctx, order, new(1))
        .await
        .expect("the first parcel");
    fulfilment::create_fulfillment(&mut tx, &ctx, order, new(1))
        .await
        .expect("the second parcel");

    let refused = fulfilment::create_fulfillment(&mut tx, &ctx, order, new(1))
        .await
        .expect_err("there were only two");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn one_shops_shipping_options_are_invisible_to_another() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    ankara_only(&mut mine, &shop.ctx()).await;
    mine.commit().await.expect("to keep the zone");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let here = DeliveryAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
        ..DeliveryAddress::default()
    };
    let offered = fulfilment::options_for(&mut theirs, &shop.theirs(), &here, None, &parcel())
        .await
        .expect("options");
    assert!(offered.is_empty(), "another shop's options were offered");

    theirs.rollback().await.expect("to roll back");
    shop.close().await;
}

/// "Standard delivery" at checkout, in whichever language the rest of the
/// page is not — falling back to the shop's own name until it is written.
#[tokio::test]
async fn a_shipping_options_name_is_localised_and_falls_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let zone = ankara_only(&mut tx, &ctx).await;
    let option = fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        NewShippingOption {
            name: "Standard delivery".into(),
            price_type: PriceKind::Flat,
            service_zone_id: zone,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("an option");

    let fallen_back = fulfilment::localised_shipping_option(&mut tx, &ctx, option.id, "tr")
        .await
        .expect("a reading");
    assert!(fallen_back.is_fallback);
    assert_eq!(fallen_back.name, "Standard delivery");

    fulfilment::put_shipping_option_translation(
        &mut tx,
        &ctx,
        option.id,
        ShippingOptionTranslation {
            shipping_option_id: option.id,
            locale: "tr".into(),
            name: "Standart teslimat".into(),
        },
    )
    .await
    .expect("a translation");

    let read = fulfilment::localised_shipping_option(&mut tx, &ctx, option.id, "tr")
        .await
        .expect("a reading");
    assert!(!read.is_fallback);
    assert_eq!(read.name, "Standart teslimat");

    assert_eq!(
        fulfilment::shipping_option_translations(&mut tx, &ctx, option.id)
            .await
            .expect("its translations")
            .len(),
        1
    );

    fulfilment::remove_shipping_option_translation(&mut tx, &ctx, option.id, "tr")
        .await
        .expect("to remove it");
    assert!(
        fulfilment::localised_shipping_option(&mut tx, &ctx, option.id, "tr")
            .await
            .expect("a reading")
            .is_fallback
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Nothing but another tenant's option id, so `tezgah_fk`'s composite key is
/// what refuses this.
#[tokio::test]
async fn a_shipping_option_translation_cannot_point_at_another_scopes_option() {
    let shop = Shop::open().await;

    let mut theirs_tx = shop.begin_as(shop.elsewhere).await;
    let their_zone = ankara_only(&mut theirs_tx, &shop.theirs()).await;
    let theirs = fulfilment::create_shipping_option(
        &mut theirs_tx,
        &shop.theirs(),
        NewShippingOption {
            name: "Their delivery".into(),
            price_type: PriceKind::Flat,
            service_zone_id: their_zone,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("an option in the other scope");
    theirs_tx.commit().await.expect("to commit");

    let mut mine = shop.begin().await;
    let refused = sqlx::query(
        "insert into shipping_option_translation (id, scope, shipping_option_id, locale, name)
         values ($1, $2, $3, 'tr', 'Standart teslimat')",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(theirs.id.as_uuid())
    .execute(&mut *mine)
    .await;
    mine.rollback().await.expect("to give the connection back");

    assert!(
        refused.is_err(),
        "a translation in one scope pointed at another scope's shipping option"
    );

    shop.close().await;
}

/// `shipping_option` has no soft delete, so this exercises the constraint
/// against a real hard delete rather than one the application never does.
#[tokio::test]
async fn a_deleted_shipping_option_takes_its_translations_with_it() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let zone = ankara_only(&mut tx, &ctx).await;
    let option = fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        NewShippingOption {
            name: "Standard delivery".into(),
            price_type: PriceKind::Flat,
            service_zone_id: zone,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("an option");

    fulfilment::put_shipping_option_translation(
        &mut tx,
        &ctx,
        option.id,
        ShippingOptionTranslation {
            shipping_option_id: option.id,
            locale: "tr".into(),
            name: "Standart teslimat".into(),
        },
    )
    .await
    .expect("a translation");

    sqlx::query("delete from shipping_option where id = $1")
        .bind(option.id.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("to delete the option");

    let left: i64 = sqlx::query_scalar(
        "select count(*) from shipping_option_translation where shipping_option_id = $1",
    )
    .bind(option.id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("to count");
    assert_eq!(left, 0, "the translation outlived the option it named");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_calculated_option_is_quoted_by_the_carrier_and_a_flat_one_is_not() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let zone = ankara_only(&mut tx, &ctx).await;
    fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        NewShippingOption {
            name: "By weight".into(),
            price_type: PriceKind::Calculated,
            service_zone_id: zone,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("a calculated option");

    let here = DeliveryAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
        ..DeliveryAddress::default()
    };

    let carrier = Carrier::default();
    let offered = fulfilment::priced_options_for(&mut tx, &ctx, &here, None, &parcel(), &carrier)
        .await
        .expect("options");
    assert_eq!(offered.len(), 2);

    let calculated = offered
        .iter()
        .find(|priced| priced.option.name == "By weight")
        .expect("the calculated option");
    assert_eq!(
        calculated.price.map(|money| money.amount),
        Some(dec!(42)),
        "the carrier was not asked what it costs"
    );

    let flat = offered
        .iter()
        .find(|priced| priced.option.name == "Next day")
        .expect("the flat option");
    assert!(
        flat.price.is_none(),
        "a flat option was priced by the carrier instead of by the shop"
    );
    assert_eq!(
        Carrier::count(&carrier.quotes),
        1,
        "the carrier was asked about an option it does not price"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn the_carrier_ships_the_parcel_and_its_tracking_number_is_kept() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let (order, item, location) = an_order(&mut tx, shop.here.0).await;
    let carrier = Carrier::default();

    let made = fulfilment::create_fulfillment_with(
        &mut tx,
        &ctx,
        order,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            address: Some(DeliveryAddress {
                country_code: "TR".into(),
                province_code: Some("06".into()),
                ..DeliveryAddress::default()
            }),
            data: None,
            items: vec![half_of(item)],
        },
        &carrier,
    )
    .await
    .expect("a fulfilment");

    assert_eq!(Carrier::count(&carrier.shipments), 1);

    let labels = fulfilment::labels(&mut tx, &ctx, order, made.id)
        .await
        .expect("its labels");
    assert_eq!(labels.len(), 1);
    assert_eq!(labels[0].tracking_number, "CARRIER-1");

    fulfilment::cancel_fulfillment_with(&mut tx, &ctx, order, made.id, &carrier)
        .await
        .expect("cancelling");
    assert_eq!(
        Carrier::count(&carrier.cancels),
        1,
        "the carrier still has a label for a parcel that will not be sent"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// A host that can grant shipping settings and parcels apart, which is the
/// whole point of them being different resources.
#[derive(Debug)]
struct Grants {
    settings: bool,
    parcels: bool,
}

impl tezgah::ports::Authorizer for Grants {
    fn authorize(
        &self,
        _: &tezgah::ports::Actor,
        _: tezgah::ports::Action,
        resource: &tezgah::ports::Resource,
    ) -> tezgah::Result<tezgah::ports::Permit> {
        let allowed = match resource {
            tezgah::ports::Resource::Shipping { .. } => self.settings,
            tezgah::ports::Resource::Fulfillment { .. } => self.parcels,
            _ => true,
        };

        if allowed {
            Ok(tezgah::ports::Permit::granted())
        } else {
            Err(tezgah::Error::denied())
        }
    }
}

impl tezgah::ports::Clock for Grants {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}

#[async_trait]
impl tezgah::ports::AuditSink for Grants {
    async fn record(&self, _: &mut Tx<'_>, _: tezgah::ports::AuditEntry) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl tezgah::ports::EventSink for Grants {
    async fn emit(&self, _: &mut Tx<'_>, _: tezgah::ports::Event) -> tezgah::Result<()> {
        Ok(())
    }
}

#[async_trait]
impl tezgah::ports::Jobs for Grants {
    async fn enqueue(&self, _: &mut Tx<'_>, _: tezgah::ports::JobSpec) -> tezgah::Result<()> {
        Ok(())
    }
}

/// A shipping option belongs to no order. Asking about one as a parcel with a
/// nil order handed an authorizer two questions it could not tell apart, so
/// "may edit shipping settings" quietly granted something else.
#[tokio::test]
async fn shipping_settings_are_not_asked_for_as_a_parcel() {
    let shop = Shop::open().await;

    let parcels = Grants {
        settings: false,
        parcels: true,
    };
    let ctx = shop.ctx_as(
        tezgah::ports::Actor::System,
        &parcels as &dyn tezgah::ports::Host,
    );
    let mut tx = shop.begin().await;
    let refused = fulfilment::create_set(
        &mut tx,
        &ctx,
        NewFulfillmentSet {
            name: "delivery".into(),
            kind: SetKind::Shipping,
        },
    )
    .await;
    tx.rollback().await.expect("to give the connection back");

    assert!(
        refused.is_err(),
        "a host granted parcels and nothing else reached the shop's shipping settings"
    );

    let settings = Grants {
        settings: true,
        parcels: false,
    };
    let ctx = shop.ctx_as(
        tezgah::ports::Actor::System,
        &settings as &dyn tezgah::ports::Host,
    );
    let mut tx = shop.begin().await;
    let allowed = fulfilment::create_set(
        &mut tx,
        &ctx,
        NewFulfillmentSet {
            name: "delivery".into(),
            kind: SetKind::Shipping,
        },
    )
    .await;
    tx.rollback().await.expect("to give the connection back");

    assert!(
        allowed.is_ok(),
        "the question was not put as a shipping one: {allowed:?}"
    );

    shop.close().await;
}

/// #181: `shipping_option_type_id` reaches the row, is read back, and can be
/// changed after the fact.
#[tokio::test]
async fn a_shipping_option_type_is_attached_and_read_back() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let zone = ankara_only(&mut tx, &ctx).await;

    let express = Uuid::now_v7();
    sqlx::query(
        "insert into shipping_option_type (id, scope, label, code)
         values ($1, $2, 'Express', 'express')",
    )
    .bind(express)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a shipping option type");

    let option = fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        NewShippingOption {
            name: "Same day".into(),
            price_type: PriceKind::Flat,
            service_zone_id: zone,
            shipping_profile_id: None,
            provider_id: None,
            shipping_option_type_id: Some(express),
            data: None,
        },
    )
    .await
    .expect("an option");
    assert_eq!(option.shipping_option_type_id, Some(express));

    let read = fulfilment::shipping_option(&mut tx, &ctx, option.id)
        .await
        .expect("the option read back");
    assert_eq!(read.shipping_option_type_id, Some(express));

    let standard = Uuid::now_v7();
    sqlx::query(
        "insert into shipping_option_type (id, scope, label, code)
         values ($1, $2, 'Standard', 'standard')",
    )
    .bind(standard)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a second type");

    let updated = fulfilment::update_shipping_option(
        &mut tx,
        &ctx,
        option.id,
        fulfilment::ShippingOptionPatch {
            shipping_option_type_id: Some(standard),
            ..fulfilment::ShippingOptionPatch::default()
        },
    )
    .await
    .expect("the edit");
    assert_eq!(updated.shipping_option_type_id, Some(standard));

    let offered = fulfilment::options_for(
        &mut tx,
        &ctx,
        &DeliveryAddress {
            country_code: "TR".into(),
            province_code: Some("06".into()),
            ..DeliveryAddress::default()
        },
        None,
        &parcel(),
    )
    .await
    .expect("options");
    assert!(
        offered
            .iter()
            .any(|row| row.id == option.id && row.shipping_option_type_id == Some(standard)),
        "checkout reads the type it was just changed to"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// #182: dropping a carrier stops its options from being offered, without
/// touching what it already shipped.
#[tokio::test]
async fn a_disabled_providers_options_stop_being_offered_but_its_shipments_are_untouched() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let mut tx = shop.begin().await;

    let zone = ankara_only(&mut tx, &ctx).await;

    let provider = fulfilment::register_provider(&mut tx, &ctx, "carrier")
        .await
        .expect("a provider");
    assert!(provider.is_enabled);

    let carried = fulfilment::create_shipping_option(
        &mut tx,
        &ctx,
        NewShippingOption {
            name: "Carrier delivery".into(),
            price_type: PriceKind::Flat,
            service_zone_id: zone,
            shipping_profile_id: None,
            provider_id: Some(provider.id),
            shipping_option_type_id: None,
            data: None,
        },
    )
    .await
    .expect("an option");

    let here = DeliveryAddress {
        country_code: "TR".into(),
        province_code: Some("06".into()),
        ..DeliveryAddress::default()
    };

    let offered = fulfilment::options_for(&mut tx, &ctx, &here, None, &parcel())
        .await
        .expect("options");
    assert!(offered.iter().any(|row| row.id == carried.id));

    let (order, item, location) = an_order(&mut tx, shop.here.0).await;
    let shipped = fulfilment::create_fulfillment(
        &mut tx,
        &ctx,
        order,
        NewFulfillment {
            location_id: location,
            shipping_option_id: Some(carried.id),
            provider_id: Some(provider.id),
            requires_shipping: true,
            created_by: Some("a test".into()),
            address: None,
            data: None,
            items: vec![half_of(item)],
        },
    )
    .await
    .expect("a fulfilment made while the carrier was on");

    let disabled = fulfilment::set_provider_enabled(&mut tx, &ctx, provider.id, false)
        .await
        .expect("to disable it");
    assert!(!disabled.is_enabled);

    let offered_now = fulfilment::options_for(&mut tx, &ctx, &here, None, &parcel())
        .await
        .expect("options");
    assert!(
        !offered_now.iter().any(|row| row.id == carried.id),
        "a dropped carrier's option stops being offered"
    );
    assert!(
        !offered_now.is_empty(),
        "ankara_only's own flat option, which has no provider, is unaffected"
    );

    let still_there = fulfilment::fulfillment(&mut tx, &ctx, order, shipped.id)
        .await
        .expect("the existing shipment is still there");
    assert_eq!(still_there.provider_id, Some(provider.id));

    let reenabled = fulfilment::set_provider_enabled(&mut tx, &ctx, provider.id, true)
        .await
        .expect("to resume offering it");
    assert!(reenabled.is_enabled);

    let offered_again = fulfilment::options_for(&mut tx, &ctx, &here, None, &parcel())
        .await
        .expect("options");
    assert!(offered_again.iter().any(|row| row.id == carried.id));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
