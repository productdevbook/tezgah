//! Fulfilment, against a real Postgres.

mod common;

use std::sync::atomic::{AtomicU32, Ordering};

use async_trait::async_trait;
use common::Shop;
use rust_decimal_macros::dec;
use tezgah::fulfilment::{
    self, DeliveryAddress, FulfillmentProvider, NewFulfillment, NewFulfillmentItem,
    NewFulfillmentSet, NewGeoZone, NewLabel, NewServiceZone, NewShippingOption, PriceKind, SetKind,
    Shipment, ShipmentRequest, Shippable, ZoneKind,
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
