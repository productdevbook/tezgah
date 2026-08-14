//! Getting the goods to somebody.
//!
//! A fulfilment set is a way of handing over — delivery, or a counter to collect
//! from. Under it sit service zones, and under those the pieces of the world
//! they cover. An address falls into a geo zone, the geo zone belongs to a
//! service zone, and the options on that service zone are what checkout may
//! offer.
//!
//! A fulfilment is one parcel. The timestamps on it are the whole state
//! machine, and the database refuses the transitions that make no sense; the
//! calls here refuse them earlier and say which one it was.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    FulfillmentId, FulfillmentSetId, GeoZoneId, InventoryItemId, LineItemId, OrderId, OrderItemId,
    SalesChannelId, ServiceZoneId, ShippingOptionId, ShippingProfileId, StockLocationId, VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

/// Shipping is configuration: providers, a set's zones, the zones an address
/// falls in and the options under them are wanted whole — an option missed
/// because it fell off a page is an option the shopper cannot choose — so each
/// is capped rather than paged.
const MAX_PROVIDERS: i64 = 200;
const MAX_SERVICE_ZONES: i64 = 500;
const MAX_GEO_ZONES: i64 = 500;
const MAX_SHIPPING_OPTIONS: i64 = 500;
const MAX_OPTION_RULES: i64 = 2_000;
/// Labels are read inside a fulfilment's detail, not browsed on their own.
const MAX_LABELS: i64 = 200;

/// Configuration belongs to the shop rather than to any one order, so the
/// order it is judged against is nil.
fn config(id: Uuid) -> Resource {
    Resource::Fulfillment {
        id,
        order: Uuid::nil(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetKind {
    Shipping,
    Pickup,
}

impl SetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SetKind::Shipping => "shipping",
            SetKind::Pickup => "pickup",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZoneKind {
    Country,
    Province,
    City,
    Zip,
}

impl ZoneKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ZoneKind::Country => "country",
            ZoneKind::Province => "province",
            ZoneKind::City => "city",
            ZoneKind::Zip => "zip",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceKind {
    /// Priced in the shop's own price list.
    Flat,
    /// Priced by the carrier at the time, so nothing is stored.
    Calculated,
}

impl PriceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PriceKind::Flat => "flat",
            PriceKind::Calculated => "calculated",
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FulfillmentProviderRow {
    pub id: Uuid,
    pub name: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FulfillmentSet {
    pub id: FulfillmentSetId,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewFulfillmentSet {
    pub name: String,
    pub kind: SetKind,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ServiceZone {
    pub id: ServiceZoneId,
    pub name: String,
    pub fulfillment_set_id: FulfillmentSetId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewServiceZone {
    pub name: String,
    pub fulfillment_set_id: FulfillmentSetId,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct GeoZone {
    pub id: GeoZoneId,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub country_code: String,
    pub province_code: Option<String>,
    pub city: Option<String>,
    /// The carrier's own postcode language. `{"prefixes": [...]}` and
    /// `{"codes": [...]}` are the two tezgah reads; anything else is the
    /// caller's to match.
    pub postal_expression: Option<serde_json::Value>,
    pub service_zone_id: ServiceZoneId,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewGeoZone {
    pub kind: ZoneKind,
    pub country_code: String,
    pub province_code: Option<String>,
    pub city: Option<String>,
    pub postal_expression: Option<serde_json::Value>,
    pub service_zone_id: ServiceZoneId,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ShippingOption {
    pub id: ShippingOptionId,
    pub name: String,
    pub price_type: String,
    pub service_zone_id: ServiceZoneId,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub provider_id: Option<Uuid>,
    pub data: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewShippingOption {
    pub name: String,
    pub price_type: PriceKind,
    pub service_zone_id: ServiceZoneId,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub provider_id: Option<Uuid>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    In,
    NotIn,
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
}

impl Operator {
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::In => "in",
            Operator::NotIn => "nin",
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::Gt => "gt",
            Operator::Gte => "gte",
            Operator::Lt => "lt",
            Operator::Lte => "lte",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "in" => Ok(Operator::In),
            "nin" => Ok(Operator::NotIn),
            "eq" => Ok(Operator::Eq),
            "ne" => Ok(Operator::Ne),
            "gt" => Ok(Operator::Gt),
            "gte" => Ok(Operator::Gte),
            "lt" => Ok(Operator::Lt),
            "lte" => Ok(Operator::Lte),
            other => Err(Error::invalid(format!("{other:?} is not an operator"))),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ShippingOptionRule {
    pub id: Uuid,
    pub attribute: String,
    pub operator: String,
    pub value: Option<serde_json::Value>,
    pub shipping_option_id: ShippingOptionId,
}

#[derive(Debug, Clone)]
pub struct NewShippingOptionRule {
    pub shipping_option_id: ShippingOptionId,
    pub attribute: String,
    pub operator: Operator,
    pub value: serde_json::Value,
}

/// Where a parcel is going, at every resolution a zone can be drawn at.
#[derive(Debug, Clone, Default)]
pub struct DeliveryAddress {
    pub country_code: String,
    pub province_code: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
}

/// What is being shipped, as far as choosing an option goes.
#[derive(Debug, Clone)]
pub struct Shippable {
    pub id: Uuid,
    pub quantity: i32,
    pub amount: Money,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub requires_shipping: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Fulfillment {
    pub id: FulfillmentId,
    pub location_id: StockLocationId,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub provider_id: Option<Uuid>,
    pub requires_shipping: bool,
    pub packed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub shipped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FulfillmentItem {
    pub id: Uuid,
    pub fulfillment_id: FulfillmentId,
    pub inventory_item_id: Option<InventoryItemId>,
    /// The order item this box holds part of. The column is called
    /// `line_item_id` because it predates the order tables.
    pub line_item_id: Option<Uuid>,
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub quantity: i32,
}

#[derive(Debug, Clone)]
pub struct NewFulfillmentItem {
    pub order_item_id: OrderItemId,
    pub inventory_item_id: Option<InventoryItemId>,
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub quantity: i32,
}

#[derive(Debug, Clone)]
pub struct NewFulfillment {
    pub location_id: StockLocationId,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub provider_id: Option<Uuid>,
    pub requires_shipping: bool,
    pub created_by: Option<String>,
    pub data: Option<serde_json::Value>,
    /// Where the parcel is going, for whoever is asked to ship it.
    pub address: Option<DeliveryAddress>,
    /// Some of the order, not necessarily all of it: a parcel that leaves today
    /// takes what is in stock today.
    pub items: Vec<NewFulfillmentItem>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FulfillmentLabel {
    pub id: Uuid,
    pub fulfillment_id: FulfillmentId,
    pub tracking_number: String,
    pub tracking_url: Option<String>,
    pub label_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewLabel {
    pub tracking_number: String,
    pub tracking_url: Option<String>,
    pub label_url: Option<String>,
}

/// What a carrier is asked, and what it says back.
#[derive(Debug, Clone)]
pub struct ShipmentRequest {
    /// `None` while an option is only being quoted: there is no parcel yet.
    pub fulfillment_id: Option<FulfillmentId>,
    pub option: Option<ShippingOptionId>,
    pub address: DeliveryAddress,
    pub items: Vec<Shippable>,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct Shipment {
    pub labels: Vec<NewLabel>,
    pub data: Option<serde_json::Value>,
}

/// A carrier, or whatever stands in for one.
#[async_trait]
pub trait FulfillmentProvider: Send + Sync {
    fn code(&self) -> &'static str;

    /// `None` when the carrier does not price, which is every flat-rate option.
    async fn price(&self, _request: &ShipmentRequest) -> Result<Option<Money>> {
        Ok(None)
    }

    async fn create_shipment(&self, request: &ShipmentRequest) -> Result<Shipment>;

    async fn cancel_shipment(&self, _request: &ShipmentRequest) -> Result<()> {
        Ok(())
    }
}

/// A shop with nobody integrated: somebody packs the box and writes the
/// tracking number in by hand, and everything above still works.
///
/// It prices nothing, so a flat option keeps the price the shop set; it ships
/// nothing, so no label and no tracking number come back; and cancelling tells
/// nobody, because nobody was told in the first place.
#[derive(Debug, Clone, Copy, Default)]
pub struct ManualFulfillment;

#[async_trait]
impl FulfillmentProvider for ManualFulfillment {
    fn code(&self) -> &'static str {
        "manual"
    }

    async fn price(&self, _request: &ShipmentRequest) -> Result<Option<Money>> {
        Ok(None)
    }

    async fn create_shipment(&self, _request: &ShipmentRequest) -> Result<Shipment> {
        Ok(Shipment::default())
    }

    async fn cancel_shipment(&self, _request: &ShipmentRequest) -> Result<()> {
        Ok(())
    }
}

pub async fn register_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<FulfillmentProviderRow> {
    let _: Permit = ctx.permit(Action::Write, config(Uuid::nil()))?;

    if name.trim().is_empty() {
        return Err(Error::invalid("a provider needs a name"));
    }

    let row = sqlx::query_as::<_, FulfillmentProviderRow>(
        "insert into fulfillment_provider (id, scope, name)
         values ($1, $2, $3)
         on conflict (scope, name) do update set is_enabled = true
         returning id, name, is_enabled",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(name.trim())
    .fetch_one(&mut **tx)
    .await?;

    Ok(row)
}

pub async fn providers(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<FulfillmentProviderRow>> {
    let _: Permit = ctx.permit(Action::View, config(Uuid::nil()))?;

    let rows = sqlx::query_as::<_, FulfillmentProviderRow>(
        "select id, name, is_enabled from fulfillment_provider
         where scope = $1 order by name limit $2",
    )
    .bind(ctx.scope.0)
    .bind(MAX_PROVIDERS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn create_shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    name: &str,
    kind: &str,
) -> Result<ShippingProfileId> {
    let _: Permit = ctx.permit(Action::Write, config(Uuid::nil()))?;

    if name.trim().is_empty() {
        return Err(Error::invalid("a shipping profile needs a name"));
    }

    let id = ShippingProfileId::new();
    sqlx::query("insert into shipping_profile (id, scope, name, type) values ($1, $2, $3, $4)")
        .bind(id.as_uuid())
        .bind(ctx.scope.0)
        .bind(name.trim())
        .bind(kind)
        .execute(&mut **tx)
        .await?;

    Ok(id)
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ShippingProfile {
    pub id: ShippingProfileId,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingProfileId,
) -> Result<ShippingProfile> {
    let _: Permit = ctx.permit(Action::View, config(Uuid::nil()))?;

    sqlx::query_as::<_, ShippingProfile>(
        "select id, name, type, created_at from shipping_profile
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("shipping profile"))
}

/// A field left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct ShippingProfilePatch {
    pub name: Option<String>,
    pub kind: Option<String>,
}

pub async fn update_shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingProfileId,
    patch: ShippingProfilePatch,
) -> Result<ShippingProfile> {
    let _: Permit = ctx.permit(Action::Write, config(Uuid::nil()))?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a shipping profile needs a name"));
    }

    sqlx::query_as::<_, ShippingProfile>(
        "update shipping_profile set
             name = coalesce($3::text, name),
             type = coalesce($4::text, type)
         where scope = $1 and id = $2
         returning id, name, type, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.kind.as_deref().map(str::trim))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("shipping profile"))
}

pub async fn create_set(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewFulfillmentSet,
) -> Result<FulfillmentSet> {
    let id = FulfillmentSetId::new();
    let _: Permit = ctx.permit(Action::Write, config(id.as_uuid()))?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a fulfilment set needs a name"));
    }

    let set = sqlx::query_as::<_, FulfillmentSet>(
        "insert into fulfillment_set (id, scope, name, type)
         values ($1, $2, $3, $4)
         returning id, name, type, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(new.kind.as_str())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "fulfillment_set",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": set.name, "type": set.kind }),
        },
    )
    .await?;

    Ok(set)
}

pub async fn sets(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<FulfillmentSet>> {
    let _: Permit = ctx.permit(Action::View, config(Uuid::nil()))?;

    let rows = sqlx::query_as::<_, FulfillmentSet>(
        "select id, name, type, created_at
         from fulfillment_set
         where scope = $1
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

pub async fn delete_set(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: FulfillmentSetId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, config(id.as_uuid()))?;

    let done = sqlx::query("delete from fulfillment_set where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("fulfilment set"));
    }

    Ok(())
}

pub async fn create_service_zone(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewServiceZone,
) -> Result<ServiceZone> {
    let id = ServiceZoneId::new();
    let _: Permit = ctx.permit(Action::Write, config(id.as_uuid()))?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a service zone needs a name"));
    }

    let zone = sqlx::query_as::<_, ServiceZone>(
        "insert into service_zone (id, scope, name, fulfillment_set_id)
         values ($1, $2, $3, $4)
         returning id, name, fulfillment_set_id, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(new.fulfillment_set_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    Ok(zone)
}

pub async fn service_zones(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    set: FulfillmentSetId,
) -> Result<Vec<ServiceZone>> {
    let _: Permit = ctx.permit(Action::View, config(set.as_uuid()))?;

    let rows = sqlx::query_as::<_, ServiceZone>(
        "select id, name, fulfillment_set_id, created_at
         from service_zone
         where scope = $1 and fulfillment_set_id = $2
         order by created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(set.as_uuid())
    .bind(MAX_SERVICE_ZONES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn create_geo_zone(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewGeoZone) -> Result<GeoZone> {
    let id = GeoZoneId::new();
    let _: Permit = ctx.permit(Action::Write, config(id.as_uuid()))?;

    let country = country_code(&new.country_code)?;
    if new.kind != ZoneKind::Country && new.province_code.is_none() {
        return Err(Error::invalid("a zone below a country needs a province"));
    }
    if new.kind == ZoneKind::City && new.city.is_none() {
        return Err(Error::invalid("a city zone needs a city"));
    }
    if new.kind == ZoneKind::Zip && new.postal_expression.is_none() {
        return Err(Error::invalid(
            "a postcode zone needs a postcode expression",
        ));
    }

    let zone = sqlx::query_as::<_, GeoZone>(
        "insert into geo_zone
             (id, scope, type, country_code, province_code, city, postal_expression,
              service_zone_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, type, country_code, province_code, city, postal_expression,
                   service_zone_id, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.kind.as_str())
    .bind(&country)
    .bind(new.province_code.as_deref())
    .bind(new.city.as_deref())
    .bind(new.postal_expression.as_ref())
    .bind(new.service_zone_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    Ok(zone)
}

/// Every zone an address falls into, most specific first.
pub async fn zones_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address: &DeliveryAddress,
) -> Result<Vec<GeoZone>> {
    let _: Permit = ctx.permit(Action::View, config(Uuid::nil()))?;

    let country = country_code(&address.country_code)?;
    let rows = sqlx::query_as::<_, GeoZone>(
        "select id, type, country_code, province_code, city, postal_expression,
                service_zone_id, created_at
         from geo_zone
         where scope = $1
           and country_code = $2
           and (
             province_code is null
             or ($3::text is not null and lower(province_code) = lower($3))
           )
           and (city is null or ($4::text is not null and lower(city) = lower($4)))
         order by case type
                    when 'zip' then 0 when 'city' then 1 when 'province' then 2 else 3
                  end,
                  created_at, id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(&country)
    .bind(address.province_code.as_deref())
    .bind(address.city.as_deref())
    .bind(MAX_GEO_ZONES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .filter(|zone| postcode_matches(zone, address.postal_code.as_deref()))
        .collect())
}

pub async fn create_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewShippingOption,
) -> Result<ShippingOption> {
    let id = ShippingOptionId::new();
    let _: Permit = ctx.permit(Action::Write, config(id.as_uuid()))?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a shipping option needs a name"));
    }

    let option = sqlx::query_as::<_, ShippingOption>(
        "insert into shipping_option
             (id, scope, name, price_type, service_zone_id, shipping_profile_id, provider_id,
              data)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, name, price_type, service_zone_id, shipping_profile_id, provider_id,
                   data, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(new.price_type.as_str())
    .bind(new.service_zone_id.as_uuid())
    .bind(new.shipping_profile_id.map(ShippingProfileId::as_uuid))
    .bind(new.provider_id)
    .bind(new.data.as_ref())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "shipping_option",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": option.name }),
        },
    )
    .await?;

    Ok(option)
}

/// A field left `None` is left alone. The service zone is not among them: an
/// option that changed zone would be offered to addresses it was never priced
/// for.
#[derive(Debug, Clone, Default)]
pub struct ShippingOptionPatch {
    pub name: Option<String>,
    pub price_type: Option<PriceKind>,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub provider_id: Option<Uuid>,
    pub data: Option<serde_json::Value>,
}

pub async fn update_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    patch: ShippingOptionPatch,
) -> Result<ShippingOption> {
    let _: Permit = ctx.permit(Action::Write, config(id.as_uuid()))?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a shipping option needs a name"));
    }

    let option = sqlx::query_as::<_, ShippingOption>(
        "update shipping_option set
             name = coalesce($3::text, name),
             price_type = coalesce($4::text, price_type),
             shipping_profile_id = coalesce($5::uuid, shipping_profile_id),
             provider_id = coalesce($6::uuid, provider_id),
             data = coalesce($7::jsonb, data)
         where scope = $1 and id = $2
         returning id, name, price_type, service_zone_id, shipping_profile_id, provider_id,
                   data, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.price_type.map(PriceKind::as_str))
    .bind(patch.shipping_profile_id.map(ShippingProfileId::as_uuid))
    .bind(patch.provider_id)
    .bind(patch.data.as_ref())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("shipping option"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "shipping_option",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": option.name }),
        },
    )
    .await?;

    Ok(option)
}

pub async fn create_shipping_option_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewShippingOptionRule,
) -> Result<ShippingOptionRule> {
    let _: Permit = ctx.permit(Action::Write, config(new.shipping_option_id.as_uuid()))?;

    if new.attribute.trim().is_empty() {
        return Err(Error::invalid("a rule needs an attribute"));
    }

    let rule = sqlx::query_as::<_, ShippingOptionRule>(
        "insert into shipping_option_rule (id, scope, attribute, operator, value,
                                           shipping_option_id)
         values ($1, $2, $3, $4, $5, $6)
         returning id, attribute, operator, value, shipping_option_id",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.attribute.trim())
    .bind(new.operator.as_str())
    .bind(&new.value)
    .bind(new.shipping_option_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    Ok(rule)
}

/// What checkout may offer for this address.
///
/// The address decides the zone, the zone decides the service zone, and the
/// options on it are then put to their own rules. An option carrying a shipping
/// profile is only offered when something being shipped shares it.
pub async fn shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
) -> Result<ShippingOption> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, ShippingOption>(
        "select id, name, price_type, service_zone_id, shipping_profile_id, provider_id,
                data, created_at
         from shipping_option
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("shipping option"))
}

pub async fn options_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address: &DeliveryAddress,
    sales_channel_id: Option<SalesChannelId>,
    items: &[Shippable],
) -> Result<Vec<ShippingOption>> {
    let _: Permit = ctx.permit(Action::View, config(Uuid::nil()))?;

    let zones = zones_for(tx, ctx, address).await?;
    if zones.is_empty() {
        return Ok(Vec::new());
    }

    let service_zones: Vec<Uuid> = zones
        .iter()
        .map(|zone| zone.service_zone_id.as_uuid())
        .collect();

    let options = sqlx::query_as::<_, ShippingOption>(
        "select id, name, price_type, service_zone_id, shipping_profile_id, provider_id,
                data, created_at
         from shipping_option
         where scope = $1 and service_zone_id = any($2)
         order by created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(&service_zones)
    .bind(MAX_SHIPPING_OPTIONS)
    .fetch_all(&mut **tx)
    .await?;

    if options.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<Uuid> = options.iter().map(|option| option.id.as_uuid()).collect();
    let rules = sqlx::query_as::<_, ShippingOptionRule>(
        "select id, attribute, operator, value, shipping_option_id
         from shipping_option_rule
         where scope = $1 and shipping_option_id = any($2)
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(&ids)
    .bind(MAX_OPTION_RULES)
    .fetch_all(&mut **tx)
    .await?;

    let context = option_context(sales_channel_id, items);
    let profiles: Vec<ShippingProfileId> = items
        .iter()
        .filter_map(|item| item.shipping_profile_id)
        .collect();

    Ok(options
        .into_iter()
        .filter(|option| match option.shipping_profile_id {
            Some(profile) => profiles.contains(&profile),
            None => true,
        })
        .filter(|option| {
            rules
                .iter()
                .filter(|rule| rule.shipping_option_id == option.id)
                .all(|rule| rule_holds(rule, &context))
        })
        .collect())
}

/// One option and what it costs, once whoever prices it has been asked.
#[derive(Debug, Clone)]
pub struct PricedOption {
    pub option: ShippingOption,
    /// `None` when the shop's own price list is the answer, which is every flat
    /// option and any calculated one the carrier declined to quote.
    pub price: Option<Money>,
}

/// [`options_for`], with the carrier asked for the ones the shop does not price
/// itself.
///
/// Every row this needs is read first and the carrier is asked afterwards, so
/// nothing is waiting on a network call with a statement half-finished.
pub async fn priced_options_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address: &DeliveryAddress,
    sales_channel_id: Option<SalesChannelId>,
    items: &[Shippable],
    provider: &dyn FulfillmentProvider,
) -> Result<Vec<PricedOption>> {
    let options = options_for(tx, ctx, address, sales_channel_id, items).await?;

    let mut priced = Vec::with_capacity(options.len());
    for option in options {
        let price = if option.price_type == PriceKind::Calculated.as_str() {
            let request = ShipmentRequest {
                fulfillment_id: None,
                option: Some(option.id),
                address: address.clone(),
                items: items.to_vec(),
                data: option.data.clone(),
            };
            provider.price(&request).await?
        } else {
            None
        };
        priced.push(PricedOption { option, price });
    }

    Ok(priced)
}

/// A parcel nobody carries: [`ManualFulfillment`] answers for it.
pub async fn create_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    new: NewFulfillment,
) -> Result<Fulfillment> {
    create_fulfillment_with(tx, ctx, order, new, &ManualFulfillment).await
}

pub async fn create_fulfillment_with(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    new: NewFulfillment,
    provider: &dyn FulfillmentProvider,
) -> Result<Fulfillment> {
    let id = FulfillmentId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    if new.items.is_empty() {
        return Err(Error::invalid("a fulfilment with nothing in it"));
    }
    if new.items.iter().any(|item| item.quantity <= 0) {
        return Err(Error::invalid("a fulfilment line needs a quantity"));
    }

    // The carrier is asked before the first row is written, so a call that
    // never answers leaves no parcel behind to reconcile.
    let shipment = if new.requires_shipping {
        let request = shipment_request(tx, ctx, id, &new).await?;
        provider.create_shipment(&request).await?
    } else {
        Shipment::default()
    };

    let data = shipment.data.clone().or_else(|| new.data.clone());

    let fulfillment = sqlx::query_as::<_, Fulfillment>(
        "insert into fulfillment
             (id, scope, location_id, shipping_option_id, provider_id, requires_shipping,
              created_by, data)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, location_id, shipping_option_id, provider_id, requires_shipping,
                   packed_at, shipped_at, delivered_at, canceled_at, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.location_id.as_uuid())
    .bind(new.shipping_option_id.map(ShippingOptionId::as_uuid))
    .bind(new.provider_id)
    .bind(new.requires_shipping)
    .bind(new.created_by.as_deref())
    .bind(data.as_ref())
    .fetch_one(&mut **tx)
    .await?;

    for item in &new.items {
        // The ledger is what refuses an over-fulfilment; no read precedes it.
        let moved = sqlx::query(
            "update order_item
             set fulfilled_quantity = fulfilled_quantity + $3
             where scope = $1
               and id = $2
               and order_id = $4
               and fulfilled_quantity + $3 <= quantity",
        )
        .bind(ctx.scope.0)
        .bind(item.order_item_id.as_uuid())
        .bind(item.quantity)
        .bind(order.as_uuid())
        .execute(&mut **tx)
        .await?;

        if moved.rows_affected() == 0 {
            return Err(Error::conflict(
                "that is more of an item than the order has left to fulfil",
            ));
        }

        // Written before the stock moves, so the lots that leave have a row to
        // be recorded against — that row is what answers a recall.
        let fulfillment_item_id = Uuid::now_v7();
        sqlx::query(
            "insert into fulfillment_item
                 (id, scope, fulfillment_id, inventory_item_id, line_item_id, title, sku,
                  barcode, quantity)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(fulfillment_item_id)
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(item.inventory_item_id.map(InventoryItemId::as_uuid))
        .bind(item.order_item_id.as_uuid())
        .bind(&item.title)
        .bind(item.sku.as_deref())
        .bind(item.barcode.as_deref())
        .bind(item.quantity)
        .execute(&mut **tx)
        .await?;

        for (line, inventory_item, units) in
            consumption(tx, ctx, item.order_item_id, item.quantity).await?
        {
            crate::inventory::fulfil_units(
                tx,
                ctx,
                line,
                inventory_item,
                new.location_id,
                units,
                Some(fulfillment_item_id),
            )
            .await?;
        }
    }

    for label in shipment.labels {
        add_label(tx, ctx, order, id, label).await?;
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "fulfillment",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "order_id": order.as_uuid(), "items": new.items.len() }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "fulfillment.created",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "order_id": order.as_uuid() }),
        },
    )
    .await?;

    Ok(fulfillment)
}

pub async fn fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Fulfillment> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    load(tx, ctx, id).await
}

pub async fn fulfillment_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Vec<FulfillmentItem>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    items_of(tx, ctx, id).await
}

pub async fn mark_packed(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Fulfillment> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    let now = ctx.now();
    let moved = sqlx::query(
        "update fulfillment
         set packed_at = $3
         where scope = $1 and id = $2 and packed_at is null and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(now)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        let current = load(tx, ctx, id).await?;
        return Err(Error::conflict(if current.canceled_at.is_some() {
            "that fulfilment was cancelled"
        } else {
            "that fulfilment was already packed"
        }));
    }

    audit_move(tx, ctx, id, "packed").await?;
    load(tx, ctx, id).await
}

pub async fn mark_shipped(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
    marked_by: Option<&str>,
) -> Result<Fulfillment> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    let now = ctx.now();
    let moved = sqlx::query(
        "update fulfillment
         set shipped_at = $3, packed_at = coalesce(packed_at, $3), marked_shipped_by = $4
         where scope = $1 and id = $2 and shipped_at is null and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(now)
    .bind(marked_by)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        let current = load(tx, ctx, id).await?;
        return Err(Error::conflict(if current.canceled_at.is_some() {
            "that fulfilment was cancelled"
        } else {
            "that fulfilment has already shipped"
        }));
    }

    move_quantities(tx, ctx, id, "shipped_quantity", "fulfilled_quantity").await?;
    audit_move(tx, ctx, id, "shipped").await?;

    ctx.emit(
        tx,
        Event {
            name: "fulfillment.shipped",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "order_id": order.as_uuid() }),
        },
    )
    .await?;

    load(tx, ctx, id).await
}

pub async fn mark_delivered(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Fulfillment> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    let now = ctx.now();
    let moved = sqlx::query(
        "update fulfillment
         set delivered_at = $3
         where scope = $1
           and id = $2
           and delivered_at is null
           and shipped_at is not null
           and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(now)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        let current = load(tx, ctx, id).await?;
        return Err(Error::conflict(if current.shipped_at.is_none() {
            "a fulfilment cannot be delivered before it has shipped"
        } else {
            "that fulfilment was already delivered"
        }));
    }

    move_quantities(tx, ctx, id, "delivered_quantity", "shipped_quantity").await?;
    audit_move(tx, ctx, id, "delivered").await?;

    ctx.emit(
        tx,
        Event {
            name: "fulfillment.delivered",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "order_id": order.as_uuid() }),
        },
    )
    .await?;

    load(tx, ctx, id).await
}

/// A parcel that has left cannot be called back: the carrier has it, and the
/// database says the same thing with a check constraint.
pub async fn cancel_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Fulfillment> {
    cancel_fulfillment_with(tx, ctx, order, id, &ManualFulfillment).await
}

pub async fn cancel_fulfillment_with(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
    provider: &dyn FulfillmentProvider,
) -> Result<Fulfillment> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    let current = load(tx, ctx, id).await?;
    if current.shipped_at.is_some() {
        return Err(Error::conflict(
            "a fulfilment that has shipped cannot be cancelled",
        ));
    }
    if current.canceled_at.is_some() {
        return Err(Error::conflict("that fulfilment was already cancelled"));
    }

    // Told before the row moves, so a carrier that refuses leaves the parcel
    // where it was rather than live at their end and cancelled at ours.
    if current.requires_shipping {
        provider
            .cancel_shipment(&ShipmentRequest {
                fulfillment_id: Some(id),
                option: current.shipping_option_id,
                address: DeliveryAddress::default(),
                items: Vec::new(),
                data: None,
            })
            .await?;
    }

    let now = ctx.now();
    let moved = sqlx::query(
        "update fulfillment
         set canceled_at = $3
         where scope = $1 and id = $2 and shipped_at is null and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(now)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(
            "that fulfilment moved on while it was being cancelled",
        ));
    }

    for item in items_of(tx, ctx, id).await? {
        let Some(order_item_id) = item.line_item_id else {
            continue;
        };
        sqlx::query(
            "update order_item
             set fulfilled_quantity = fulfilled_quantity - $3
             where scope = $1 and id = $2 and fulfilled_quantity >= $3",
        )
        .bind(ctx.scope.0)
        .bind(order_item_id)
        .bind(item.quantity)
        .execute(&mut **tx)
        .await?;

        let order_item_id = OrderItemId::from_uuid(order_item_id);
        for (line, inventory_item, units) in
            consumption(tx, ctx, order_item_id, item.quantity).await?
        {
            crate::inventory::unfulfil_units(
                tx,
                ctx,
                line,
                inventory_item,
                current.location_id,
                units,
                Some(item.id),
            )
            .await?;
        }
    }

    audit_move(tx, ctx, id, "cancelled").await?;

    ctx.emit(
        tx,
        Event {
            name: "fulfillment.canceled",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "order_id": order.as_uuid() }),
        },
    )
    .await?;

    load(tx, ctx, id).await
}

pub async fn add_label(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
    new: NewLabel,
) -> Result<FulfillmentLabel> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    if new.tracking_number.trim().is_empty() {
        return Err(Error::invalid("a label needs a tracking number"));
    }

    let label = sqlx::query_as::<_, FulfillmentLabel>(
        "insert into fulfillment_label
             (id, scope, fulfillment_id, tracking_number, tracking_url, label_url)
         select $1::uuid, $2::uuid, f.id, $4::text, $5::text, $6::text
         from fulfillment f
         where f.scope = $2 and f.id = $3
         returning id, fulfillment_id, tracking_number, tracking_url, label_url",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(new.tracking_number.trim())
    .bind(new.tracking_url.as_deref())
    .bind(new.label_url.as_deref())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("fulfilment"))?;

    Ok(label)
}

pub async fn labels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
) -> Result<Vec<FulfillmentLabel>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Fulfillment {
            id: id.as_uuid(),
            order: order.as_uuid(),
        },
    )?;

    let rows = sqlx::query_as::<_, FulfillmentLabel>(
        "select id, fulfillment_id, tracking_number, tracking_url, label_url
         from fulfillment_label
         where scope = $1 and fulfillment_id = $2
         order by id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(MAX_LABELS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

async fn load(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: FulfillmentId) -> Result<Fulfillment> {
    sqlx::query_as::<_, Fulfillment>(
        "select id, location_id, shipping_option_id, provider_id, requires_shipping,
                packed_at, shipped_at, delivered_at, canceled_at, created_at
         from fulfillment
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("fulfilment"))
}

async fn items_of(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentId,
) -> Result<Vec<FulfillmentItem>> {
    let rows = sqlx::query_as::<_, FulfillmentItem>(
        "select id, fulfillment_id, inventory_item_id, line_item_id, title, sku, barcode,
                quantity
         from fulfillment_item
         where scope = $1 and fulfillment_id = $2
         order by id",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// What the carrier is told about a parcel that does not exist yet: its id is
/// known before the insert, so a label can be matched back to it.
async fn shipment_request(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentId,
    new: &NewFulfillment,
) -> Result<ShipmentRequest> {
    let ids: Vec<Uuid> = new
        .items
        .iter()
        .map(|item| item.order_item_id.as_uuid())
        .collect();

    let priced: Vec<(Uuid, Option<Decimal>, String)> = sqlx::query_as(
        "select id, unit_price, currency_code::text from order_item
         where scope = $1 and id = any($2)",
    )
    .bind(ctx.scope.0)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await?;

    let mut items = Vec::with_capacity(new.items.len());
    for item in &new.items {
        let found = priced
            .iter()
            .find(|(row, _, _)| *row == item.order_item_id.as_uuid())
            .ok_or_else(|| Error::not_found("order item"))?;

        items.push(Shippable {
            id: item.order_item_id.as_uuid(),
            quantity: item.quantity,
            amount: Money::new(
                found.1.unwrap_or_default() * Decimal::from(item.quantity),
                Currency::parse(&found.2)?,
            ),
            shipping_profile_id: None,
            requires_shipping: new.requires_shipping,
        });
    }

    Ok(ShipmentRequest {
        fulfillment_id: Some(id),
        option: new.shipping_option_id,
        address: new.address.clone().unwrap_or_default(),
        items,
        data: new.data.clone(),
    })
}

/// What shipping `units` of one order item takes off the shelves: the order's
/// line, an inventory item, and how much of it one line unit consumes.
///
/// An order item whose line names no variant — a custom line typed in the back
/// office — consumes nothing, and this is empty for it.
async fn consumption(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_item_id: OrderItemId,
    units: i32,
) -> Result<Vec<(LineItemId, InventoryItemId, i32)>> {
    let line: Option<(Uuid, Option<Uuid>)> = sqlx::query_as(
        "select oi.order_line_item_id, li.variant_id
         from order_item oi
         join order_line_item li on li.scope = oi.scope and li.id = oi.order_line_item_id
         where oi.scope = $1 and oi.id = $2",
    )
    .bind(ctx.scope.0)
    .bind(order_item_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some((line_item_id, Some(variant_id))) = line else {
        return Ok(Vec::new());
    };

    let items =
        crate::inventory::inventory_items_for_variant(tx, ctx, VariantId::from_uuid(variant_id))
            .await?;

    Ok(items
        .into_iter()
        .map(|item| {
            (
                LineItemId::from_uuid(line_item_id),
                item.inventory_item_id,
                units * item.required_quantity,
            )
        })
        .collect())
}

/// Whether any of an order's parcels has left. A fulfilment has no `order_id`
/// of its own; the order is reached through what is in the box.
pub async fn anything_shipped(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order: OrderId) -> Result<bool> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Fulfillment {
            id: Uuid::nil(),
            order: order.as_uuid(),
        },
    )?;

    let shipped: Option<i32> = sqlx::query_scalar(
        "select 1 from fulfillment f
         join fulfillment_item fi on fi.scope = f.scope and fi.fulfillment_id = f.id
         join order_item oi on oi.scope = fi.scope and oi.id = fi.line_item_id
         where f.scope = $1 and oi.order_id = $2 and f.shipped_at is not null
         limit 1",
    )
    .bind(ctx.scope.0)
    .bind(order.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(shipped.is_some())
}

/// Cancels every parcel of an order that has not left, putting the stock back.
/// Returns how many there were, so nothing here hands back a list.
pub async fn cancel_open_fulfillments(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
) -> Result<u64> {
    let open: Vec<FulfillmentId> = sqlx::query_scalar(
        "select distinct f.id from fulfillment f
         join fulfillment_item fi on fi.scope = f.scope and fi.fulfillment_id = f.id
         join order_item oi on oi.scope = fi.scope and oi.id = fi.line_item_id
         where f.scope = $1 and oi.order_id = $2
           and f.shipped_at is null and f.canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(order.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for id in &open {
        cancel_fulfillment(tx, ctx, order, *id).await?;
    }

    Ok(open.len() as u64)
}

/// Moves one of the order item's counters up by what this parcel holds, never
/// past the counter above it.
async fn move_quantities(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentId,
    column: &str,
    ceiling: &str,
) -> Result<()> {
    for item in items_of(tx, ctx, id).await? {
        let Some(order_item_id) = item.line_item_id else {
            continue;
        };

        // Both names are chosen here rather than sent, so nothing a caller
        // wrote reaches the statement.
        let statement = format!(
            "update order_item
             set {column} = {column} + $3
             where scope = $1 and id = $2 and {column} + $3 <= {ceiling}"
        );

        let moved = sqlx::query(&statement)
            .bind(ctx.scope.0)
            .bind(order_item_id)
            .bind(item.quantity)
            .execute(&mut **tx)
            .await?;

        if moved.rows_affected() == 0 {
            return Err(Error::conflict("that item has already moved that far"));
        }
    }

    Ok(())
}

async fn audit_move(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentId,
    what: &'static str,
) -> Result<()> {
    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "fulfillment",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "state": what }),
        },
    )
    .await
}

fn option_context(
    sales_channel_id: Option<SalesChannelId>,
    items: &[Shippable],
) -> serde_json::Map<String, serde_json::Value> {
    let total: Decimal = items.iter().map(|item| item.amount.amount).sum();
    let count: i64 = items.iter().map(|item| i64::from(item.quantity)).sum();

    let mut context = serde_json::Map::new();
    context.insert(
        "sales_channel_id".into(),
        match sales_channel_id {
            Some(id) => serde_json::Value::String(id.to_string()),
            None => serde_json::Value::Null,
        },
    );
    context.insert(
        "item_total".into(),
        serde_json::Value::String(total.to_string()),
    );
    context.insert("item_count".into(), serde_json::json!(count));
    context.insert(
        "requires_shipping".into(),
        serde_json::json!(items.iter().any(|item| item.requires_shipping)),
    );
    context
}

fn rule_holds(
    rule: &ShippingOptionRule,
    context: &serde_json::Map<String, serde_json::Value>,
) -> bool {
    let Ok(operator) = Operator::parse(&rule.operator) else {
        return false;
    };
    let Some(actual) = context.get(&rule.attribute) else {
        return false;
    };
    let Some(expected) = rule.value.as_ref() else {
        return false;
    };

    match operator {
        Operator::Eq => same(actual, expected),
        Operator::Ne => !same(actual, expected),
        Operator::In => contains(expected, actual),
        Operator::NotIn => !contains(expected, actual),
        Operator::Gt | Operator::Gte | Operator::Lt | Operator::Lte => {
            match (number(actual), number(expected)) {
                (Some(left), Some(right)) => match operator {
                    Operator::Gt => left > right,
                    Operator::Gte => left >= right,
                    Operator::Lt => left < right,
                    _ => left <= right,
                },
                _ => false,
            }
        }
    }
}

fn same(left: &serde_json::Value, right: &serde_json::Value) -> bool {
    match (number(left), number(right)) {
        (Some(left), Some(right)) => left == right,
        _ => text(left) == text(right),
    }
}

fn contains(list: &serde_json::Value, value: &serde_json::Value) -> bool {
    match list {
        serde_json::Value::Array(items) => items.iter().any(|item| same(item, value)),
        other => same(other, value),
    }
}

fn number(value: &serde_json::Value) -> Option<Decimal> {
    match value {
        serde_json::Value::Number(number) => number.as_f64().and_then(Decimal::from_f64_retain),
        serde_json::Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

fn text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// `{"prefixes": [...]}` and `{"codes": [...]}` are what tezgah reads; a zone
/// expressed any other way matches only when a postcode was given at all.
fn postcode_matches(zone: &GeoZone, postal_code: Option<&str>) -> bool {
    let Some(expression) = zone.postal_expression.as_ref() else {
        return true;
    };
    let Some(code) = postal_code else {
        return false;
    };
    let code = code.trim().to_ascii_uppercase();

    let list = |name: &str| -> Vec<String> {
        expression
            .get(name)
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .map(|item| item.trim().to_ascii_uppercase())
                    .collect()
            })
            .unwrap_or_default()
    };

    let prefixes = list("prefixes");
    let codes = list("codes");
    if prefixes.is_empty() && codes.is_empty() {
        return true;
    }

    codes.contains(&code.to_string()) || prefixes.iter().any(|prefix| code.starts_with(prefix))
}

fn country_code(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(Error::invalid(format!("{text:?} is not a country code")));
    }
    Ok(trimmed.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(expression: Option<serde_json::Value>) -> GeoZone {
        GeoZone {
            id: GeoZoneId::new(),
            kind: "zip".into(),
            country_code: "TR".into(),
            province_code: Some("06".into()),
            city: None,
            postal_expression: expression,
            service_zone_id: ServiceZoneId::new(),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_postcode_prefix_matches_the_codes_under_it() {
        let zone = zone(Some(serde_json::json!({ "prefixes": ["06"] })));
        assert!(postcode_matches(&zone, Some("06010")));
        assert!(!postcode_matches(&zone, Some("34000")));
        assert!(!postcode_matches(&zone, None));
    }

    #[test]
    fn a_zone_without_a_postcode_expression_takes_anything() {
        assert!(postcode_matches(&zone(None), None));
    }

    #[test]
    fn a_rule_whose_attribute_is_unknown_refuses_the_option() {
        let rule = ShippingOptionRule {
            id: Uuid::now_v7(),
            attribute: "nothing_knows_this".into(),
            operator: "eq".into(),
            value: Some(serde_json::json!("x")),
            shipping_option_id: ShippingOptionId::new(),
        };
        assert!(!rule_holds(&rule, &serde_json::Map::new()));
    }
}
