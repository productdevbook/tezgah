//! What is held, where it is held, and what has been promised out of it.
//!
//! Three counts live on a level and only two of them are written: `stocked` is
//! what is on the shelf, `reserved` is what has been promised, and `available`
//! is the difference, computed by the database so no writer can disagree with
//! it.
//!
//! Reserving is the one call in this crate that two people reach at the same
//! moment, and it is written as a single conditional `update` for that reason.
//! The row lock Postgres takes for the update is what makes the loser of the
//! race see the winner's count rather than the count they both read; a `select`
//! followed by an `update` reads before the lock and sells the same unit twice.
//!
//! An item may also ask to be counted as lots rather than as a number, and
//! then the level stays the total while the lots underneath it carry the code,
//! the expiry and the day the goods arrived. Nothing about an item that did
//! not ask changes: `tracking_mode` is `quantity` until somebody says
//! otherwise, and every lot statement below is skipped for it.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{
    FulfillmentId, InventoryItemId, InventoryLevelId, InventoryLotId, LineItemId, OrderId,
    ReservationId, SalesChannelId, StockLocationId, StockTransferId, VariantId,
};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const LEVEL_COLUMNS: &str = "id, inventory_item_id, location_id, stocked_quantity,
     reserved_quantity, incoming_quantity, available_quantity, created_at";

/// What one variant is made of: a bundle, not a customer's data, and every
/// part of it is needed to reserve stock — so a ceiling rather than a page.
const MAX_ITEMS_PER_VARIANT: i64 = 200;

const RESERVATION_COLUMNS: &str = "id, inventory_item_id, location_id, quantity,
     coalesce(cart_line_item_id, order_line_item_id) as line_item_id,
     allows_backorder, expires_at, created_at";

const STOCK_TRANSFER_COLUMNS: &str = "id, inventory_item_id, from_location_id, to_location_id,
     quantity, status, reason, created_at";

/// Somewhere stock is held: a warehouse, a shop floor, a third party's shelf.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StockLocation {
    pub id: StockLocationId,
    pub name: String,
    pub address_id: Option<uuid::Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewStockLocation {
    pub name: String,
    pub address: Option<NewStockLocationAddress>,
}

/// Where a location is, which is where a supply leaves from.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StockLocationAddress {
    pub id: uuid::Uuid,
    pub address_1: String,
    pub address_2: Option<String>,
    pub company: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub phone: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct NewStockLocationAddress {
    pub address_1: String,
    pub address_2: Option<String>,
    pub company: Option<String>,
    pub city: Option<String>,
    pub country_code: String,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub phone: Option<String>,
}

const STOCK_LOCATION_ADDRESS_COLUMNS: &str = "id, address_1, address_2, company, city,
     country_code, province, postal_code, phone";

fn country_code(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(Error::invalid("a country code is two letters"));
    }
    Ok(trimmed.to_ascii_uppercase())
}

async fn write_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewStockLocationAddress,
) -> Result<StockLocationAddress> {
    if new.address_1.trim().is_empty() {
        return Err(Error::invalid("an address needs a first line"));
    }
    let country = country_code(&new.country_code)?;

    Ok(sqlx::query_as::<_, StockLocationAddress>(&format!(
        "insert into stock_location_address
             (id, scope, address_1, address_2, company, city, country_code, province,
              postal_code, phone)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning {STOCK_LOCATION_ADDRESS_COLUMNS}"
    ))
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.address_1.trim())
    .bind(new.address_2)
    .bind(new.company)
    .bind(new.city)
    .bind(&country)
    .bind(new.province)
    .bind(new.postal_code)
    .bind(new.phone)
    .fetch_one(&mut **tx)
    .await?)
}

/// What is counted, which is not what is sold: several variants may be the same
/// physical thing, and one variant may consume several of these.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct InventoryItem {
    pub id: InventoryItemId,
    pub sku: Option<String>,
    pub title: Option<String>,
    pub requires_shipping: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewInventoryItem {
    pub sku: Option<String>,
    pub title: Option<String>,
    pub requires_shipping: bool,
}

/// One item's count at one location.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct InventoryLevel {
    pub id: InventoryLevelId,
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub stocked_quantity: i32,
    pub reserved_quantity: i32,
    pub incoming_quantity: i32,
    /// `stocked - reserved`, and negative when a backorder was allowed.
    pub available_quantity: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A claim on stock rather than a movement of it.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Reservation {
    pub id: ReservationId,
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub quantity: i32,
    pub line_item_id: Option<LineItemId>,
    pub allows_backorder: bool,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// A move between two locations, done in one transaction: `stock_transfer` is
/// what answers "where did it go" that a pair of `adjust_stock` calls could
/// not. `status` is `completed` the moment [`transfer_stock`] returns — the
/// move itself is atomic, so there is no observable moment the units belong
/// to neither location — and `cancelled` is left for a future compensation.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StockTransfer {
    pub id: StockTransferId,
    pub inventory_item_id: InventoryItemId,
    pub from_location_id: StockLocationId,
    pub to_location_id: StockLocationId,
    pub quantity: i32,
    pub status: String,
    pub reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// How much of an item one of a variant consumes. Several rows for one variant
/// is a bundle.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct VariantInventoryItem {
    pub variant_id: VariantId,
    pub inventory_item_id: InventoryItemId,
    pub required_quantity: i32,
}

fn positive(quantity: i32, what: &'static str) -> Result<i32> {
    if quantity <= 0 {
        return Err(Error::invalid(format!("{what} must be more than none")));
    }
    Ok(quantity)
}

pub async fn create_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewStockLocation,
) -> Result<StockLocation> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a stock location needs a name"));
    }

    let id = StockLocationId::new();
    let address_id = match new.address {
        Some(address) => Some(write_address(tx, ctx, address).await?.id),
        None => None,
    };
    // `on conflict` rather than catching the violation after it happened: a
    // caught unique violation has already aborted the caller's transaction.
    let location = sqlx::query_as::<_, StockLocation>(
        "insert into stock_location (id, scope, name, address_id)
         values ($1, $2, $3, $4)
         on conflict (scope, name) do nothing
         returning id, name, address_id, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(address_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("a stock location by that name is already here"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "stock_location",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": location.name }),
        },
    )
    .await?;

    Ok(location)
}

pub async fn stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<StockLocation> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, StockLocation>(
        "select id, name, address_id, created_at
         from stock_location
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("stock location"))
}

pub async fn stock_locations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<StockLocation>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, StockLocation>(
        "select id, name, address_id, created_at
         from stock_location
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

/// Gives a location its address, or replaces the one it has.
///
/// Replaced in place rather than by a second row: a location has one address,
/// and the despatch address of an order that already shipped is on the order.
pub async fn set_stock_location_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    new: NewStockLocationAddress,
) -> Result<StockLocationAddress> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    let existing = sqlx::query_as::<_, (Option<uuid::Uuid>,)>(
        "select address_id from stock_location where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("stock location"))?
    .0;

    let address = match existing {
        Some(address_id) => {
            let country = country_code(&new.country_code)?;
            if new.address_1.trim().is_empty() {
                return Err(Error::invalid("an address needs a first line"));
            }
            sqlx::query_as::<_, StockLocationAddress>(&format!(
                "update stock_location_address set
                     address_1 = $3, address_2 = $4, company = $5, city = $6,
                     country_code = $7, province = $8, postal_code = $9, phone = $10
                 where scope = $1 and id = $2
                 returning {STOCK_LOCATION_ADDRESS_COLUMNS}"
            ))
            .bind(ctx.scope.0)
            .bind(address_id)
            .bind(new.address_1.trim())
            .bind(new.address_2)
            .bind(new.company)
            .bind(new.city)
            .bind(&country)
            .bind(new.province)
            .bind(new.postal_code)
            .bind(new.phone)
            .fetch_optional(&mut **tx)
            .await?
            .ok_or_else(|| Error::not_found("stock location address"))?
        }
        None => {
            let written = write_address(tx, ctx, new).await?;
            sqlx::query("update stock_location set address_id = $3 where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(id.as_uuid())
                .bind(written.id)
                .execute(&mut **tx)
                .await?;
            written
        }
    };

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "stock_location_address",
            entity_id: address.id,
            summary: serde_json::json!({ "country_code": address.country_code }),
        },
    )
    .await?;

    Ok(address)
}

pub async fn stock_location_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<Option<StockLocationAddress>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    Ok(sqlx::query_as::<_, StockLocationAddress>(
        "select a.id, a.address_1, a.address_2, a.company, a.city, a.country_code,
                a.province, a.postal_code, a.phone
         from stock_location_address a
         join stock_location l on l.scope = a.scope and l.address_id = a.id
         where a.scope = $1 and l.id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?)
}

/// Where a supply leaves from, when nothing else says.
///
/// The oldest location that has an address, because a shop that keeps stock in
/// two countries has to say where it is registered rather than let the tax
/// decision be settled by which warehouse was made first.
pub async fn origin_country(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Option<String>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    Ok(sqlx::query_as::<_, (String,)>(
        "select a.country_code
             from stock_location l
             join stock_location_address a on a.scope = l.scope and a.id = l.address_id
             where l.scope = $1
             order by l.created_at, l.id
             limit 1",
    )
    .bind(ctx.scope.0)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| row.0))
}

pub async fn rename_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    name: &str,
) -> Result<StockLocation> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    if name.trim().is_empty() {
        return Err(Error::invalid("a stock location needs a name"));
    }

    let location = sqlx::query_as::<_, StockLocation>(
        "update stock_location set name = $3
         where scope = $1 and id = $2
           and not exists (
               select 1 from stock_location taken
               where taken.scope = $1 and taken.name = $3 and taken.id <> $2
           )
         returning id, name, address_id, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await?;

    let Some(location) = location else {
        return Err(location_missing_or(
            tx,
            ctx,
            id,
            Error::conflict("a stock location by that name is already here"),
        )
        .await);
    };

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "stock_location",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": location.name }),
        },
    )
    .await?;

    Ok(location)
}

pub async fn delete_stock_location(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    // The condition is in the statement rather than in a caught foreign key
    // violation, so a location that still holds stock is a refusal the caller
    // can carry on from instead of an aborted transaction.
    let done = sqlx::query(
        "delete from stock_location
         where scope = $1 and id = $2
           and not exists (select 1 from inventory_level where scope = $1 and location_id = $2)
           and not exists (select 1 from inventory_lot where scope = $1 and location_id = $2)
           and not exists (select 1 from reservation_item where scope = $1 and location_id = $2)
           and not exists (select 1 from fulfillment where scope = $1 and location_id = $2)",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(location_missing_or(
            tx,
            ctx,
            id,
            Error::conflict("stock is still counted at that location"),
        )
        .await);
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "stock_location",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

/// A location a channel may ship from. An unlisted location is invisible to
/// that channel rather than merely unpreferred.
pub async fn link_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    location_id: StockLocationId,
    sales_channel_id: SalesChannelId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(location_id.as_uuid()),
        },
    )?;

    // Asked rather than inferred from a violation: a link nothing can be made
    // of is a missing parent, which is a `not_found`, and it used to be
    // reported as a conflict alongside two unrelated conditions.
    let (location_here, channel_here): (bool, bool) = sqlx::query_as(
        "select exists (select 1 from stock_location where scope = $1 and id = $2),
                exists (select 1 from sales_channel where scope = $1 and id = $3)",
    )
    .bind(ctx.scope.0)
    .bind(location_id.as_uuid())
    .bind(sales_channel_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if !location_here {
        return Err(Error::not_found("stock location"));
    }
    if !channel_here {
        return Err(Error::not_found("sales channel"));
    }

    sqlx::query(
        "insert into stock_location_sales_channel
             (id, scope, stock_location_id, sales_channel_id)
         values ($1, $2, $3, $4)
         on conflict (scope, stock_location_id, sales_channel_id) do nothing",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(location_id.as_uuid())
    .bind(sales_channel_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "stock_location_sales_channel",
            entity_id: location_id.as_uuid(),
            summary: serde_json::json!({ "sales_channel_id": sales_channel_id }),
        },
    )
    .await?;

    Ok(())
}

pub async fn unlink_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    location_id: StockLocationId,
    sales_channel_id: SalesChannelId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Inventory {
            id: Some(location_id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "delete from stock_location_sales_channel
         where scope = $1 and stock_location_id = $2 and sales_channel_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(location_id.as_uuid())
    .bind(sales_channel_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("stock location sales channel"));
    }

    Ok(())
}

pub async fn locations_for_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    sales_channel_id: SalesChannelId,
    paging: Paging,
) -> Result<Page<StockLocation>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, StockLocation>(
        "select l.id, l.name, l.address_id, l.created_at
         from stock_location l
         join stock_location_sales_channel link
           on link.scope = $1
          and link.stock_location_id = l.id
          and link.sales_channel_id = $2
         where l.scope = $1
           and ($3::timestamptz is null or (l.created_at, l.id) > ($3, $4))
         order by l.created_at, l.id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(sales_channel_id.as_uuid())
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

pub async fn create_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewInventoryItem,
) -> Result<InventoryItem> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    let id = InventoryItemId::new();
    let item = sqlx::query_as::<_, InventoryItem>(
        "insert into inventory_item (id, scope, sku, title, requires_shipping)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, sku) where deleted_at is null do nothing
         returning id, sku, title, requires_shipping, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.sku.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(new.title.as_deref())
    .bind(new.requires_shipping)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that sku is already counted here"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_item",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "sku": item.sku }),
        },
    )
    .await?;

    Ok(item)
}

pub async fn inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
) -> Result<InventoryItem> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, InventoryItem>(
        "select id, sku, title, requires_shipping, created_at
         from inventory_item
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("inventory item"))
}

pub async fn inventory_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<InventoryItem>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, InventoryItem>(
        "select id, sku, title, requires_shipping, created_at
         from inventory_item
         where scope = $1
           and deleted_at is null
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

/// Soft, because an item is what past reservations and past orders point at.
pub async fn delete_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "update inventory_item set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("inventory item"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "inventory_item",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

/// One of a variant consumes `required_quantity` of an item. Two calls with two
/// items make the variant a bundle.
pub async fn attach_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    inventory_item_id: InventoryItemId,
    required_quantity: i32,
) -> Result<VariantInventoryItem> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let required_quantity = positive(required_quantity, "a required quantity")?;

    let link = sqlx::query_as::<_, VariantInventoryItem>(
        "insert into variant_inventory_item
             (id, scope, variant_id, inventory_item_id, required_quantity)
         select $1, $2, $3, $4, $5
         where exists (select 1 from product_variant where scope = $2 and id = $3)
           and exists (select 1 from inventory_item
                       where scope = $2 and id = $4 and deleted_at is null)
         on conflict (scope, variant_id, inventory_item_id)
         do update set required_quantity = excluded.required_quantity
         returning variant_id, inventory_item_id, required_quantity",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(required_quantity)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("variant or inventory item"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "variant_inventory_item",
            entity_id: variant_id.as_uuid(),
            summary: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "required_quantity": required_quantity,
            }),
        },
    )
    .await?;

    Ok(link)
}

pub async fn detach_inventory_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    inventory_item_id: InventoryItemId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "delete from variant_inventory_item
         where scope = $1 and variant_id = $2 and inventory_item_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("variant inventory item"));
    }

    Ok(())
}

pub async fn inventory_items_for_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<VariantInventoryItem>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, VariantInventoryItem>(
        "select variant_id, inventory_item_id, required_quantity
         from variant_inventory_item
         where scope = $1 and variant_id = $2
         order by inventory_item_id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(MAX_ITEMS_PER_VARIANT)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// Starts, or replaces, what is held of an item at a location.
pub async fn set_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    stocked_quantity: i32,
    incoming_quantity: i32,
) -> Result<InventoryLevel> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    if stocked_quantity < 0 || incoming_quantity < 0 {
        return Err(Error::invalid("a stock count cannot be below none"));
    }

    if tracking(tx, ctx, inventory_item_id).await?.mode.by_lot() {
        return Err(Error::invalid(
            "a lot-tracked item is stocked by receiving lots, not by setting a total",
        ));
    }

    let level = sqlx::query_as::<_, InventoryLevel>(&format!(
        "insert into inventory_level
             (id, scope, inventory_item_id, location_id, stocked_quantity, incoming_quantity)
         select $1, $2, $3, $4, $5, $6
         where exists (select 1 from inventory_item where scope = $2 and id = $3)
           and exists (select 1 from stock_location where scope = $2 and id = $4)
         on conflict (scope, inventory_item_id, location_id)
         do update set stocked_quantity = excluded.stocked_quantity,
                       incoming_quantity = excluded.incoming_quantity
         returning {LEVEL_COLUMNS}"
    ))
    .bind(InventoryLevelId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(stocked_quantity)
    .bind(incoming_quantity)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("inventory item or stock location"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_level",
            entity_id: level.id.as_uuid(),
            summary: serde_json::json!({
                "stocked_quantity": level.stocked_quantity,
                "incoming_quantity": level.incoming_quantity,
            }),
        },
    )
    .await?;

    Ok(level)
}

pub async fn level(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
) -> Result<InventoryLevel> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, InventoryLevel>(&format!(
        "select {LEVEL_COLUMNS}
         from inventory_level
         where scope = $1 and inventory_item_id = $2 and location_id = $3"
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("inventory level"))
}

pub async fn levels_for_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    paging: Paging,
) -> Result<Page<InventoryLevel>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, InventoryLevel>(&format!(
        "select {LEVEL_COLUMNS}
         from inventory_level
         where scope = $1
           and inventory_item_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
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

/// A count done by hand: a breakage, a delivery, a recount. `delta` may be
/// negative, and the row refuses to go below none.
pub async fn adjust_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    delta: i32,
    reason: Option<&str>,
) -> Result<InventoryLevel> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    if delta == 0 {
        return Err(Error::invalid("an adjustment of none changes nothing"));
    }

    if tracking(tx, ctx, inventory_item_id).await?.mode.by_lot() {
        return Err(Error::invalid(
            "a lot-tracked item is counted lot by lot; adjust the lot",
        ));
    }

    let level = sqlx::query_as::<_, InventoryLevel>(&format!(
        "update inventory_level
         set stocked_quantity = stocked_quantity + $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and stocked_quantity + $4 >= 0
         returning {LEVEL_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(delta)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(level) = level else {
        return Err(level_missing_or(
            tx,
            ctx,
            inventory_item_id,
            location_id,
            Error::conflict("that would leave less than none in stock"),
        )
        .await);
    };

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_level",
            entity_id: level.id.as_uuid(),
            summary: serde_json::json!({
                "delta": delta,
                "reason": reason,
                "stocked_quantity": level.stocked_quantity,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.adjusted",
            entity_id: level.id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "delta": delta,
                "stocked_quantity": level.stocked_quantity,
            }),
        },
    )
    .await?;

    // A count is the truth, so it is not refused for disagreeing with what was
    // already promised — but somebody has to be told that stock is owed which
    // no longer exists, and this is the only place it can happen without a
    // backorder having been asked for.
    if level.available_quantity < 0 {
        ctx.emit(
            tx,
            Event {
                name: "stock.oversold",
                entity_id: level.id.as_uuid(),
                payload: serde_json::json!({
                    "inventory_item_id": inventory_item_id,
                    "location_id": location_id,
                    "stocked_quantity": level.stocked_quantity,
                    "reserved_quantity": level.reserved_quantity,
                    "short_by": -level.available_quantity,
                }),
            },
        )
        .await?;
    }

    Ok(level)
}

/// Moves stock from one location to another as one act.
///
/// The decrement and the increment land in this one transaction, so a
/// reconciler is never asked to find the other half of a move that half
/// happened — the two adjustments a transfer used to be, with nothing to say
/// they belonged together. The `stock_transfer` row this writes is that
/// belonging: `from`, `to`, the item, the quantity, and the one movement an
/// audit now shows instead of two adjustments (#147).
///
/// There is no third, in-transit level column. The move is one transaction,
/// so at no committed instant are the units missing from both locations or
/// counted at both — the row lock on the source's conditional update is what
/// makes that true for two transfers reaching for the same last unit, the
/// same way [`adjust_stock`] and [`reserve`] already rely on it. What the
/// transfer row is for is answering *which* units moved and *where*, after
/// the fact — `inventory_level` is left exactly as it was.
///
/// A lot-tracked item moves specific lots, picked in the item's own
/// allocation order (FEFO or FIFO) by [`transfer_lots`]. `stock_transfer_lot`
/// is the receiving location's copy of which lots arrived, the way
/// `fulfillment_lot` is a parcel's.
pub async fn transfer_stock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    from_location_id: StockLocationId,
    to_location_id: StockLocationId,
    quantity: i32,
    reason: Option<&str>,
) -> Result<StockTransfer> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let quantity = positive(quantity, "a transferred quantity")?;
    if from_location_id == to_location_id {
        return Err(Error::invalid("a transfer needs two different locations"));
    }

    let (item_here, from_here, to_here): (bool, bool, bool) = sqlx::query_as(
        "select exists (select 1 from inventory_item
                        where scope = $1 and id = $2 and deleted_at is null),
                exists (select 1 from stock_location where scope = $1 and id = $3),
                exists (select 1 from stock_location where scope = $1 and id = $4)",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(from_location_id.as_uuid())
    .bind(to_location_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if !item_here {
        return Err(Error::not_found("inventory item"));
    }
    if !from_here || !to_here {
        return Err(Error::not_found("stock location"));
    }

    let tracking = tracking(tx, ctx, inventory_item_id).await?;

    // What is promised at the source may not leave on a truck: the condition
    // is `available`, not `stocked`, so a reservation at the source location
    // is not shipped out from under the sale it was made for.
    let shipped = sqlx::query(
        "update inventory_level
         set stocked_quantity = stocked_quantity - $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and stocked_quantity - reserved_quantity >= $4",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(from_location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    if shipped.rows_affected() == 0 {
        return Err(level_missing_or(
            tx,
            ctx,
            inventory_item_id,
            from_location_id,
            Error::conflict("there is not that much unpromised stock to transfer"),
        )
        .await);
    }

    let lots = if tracking.mode.by_lot() {
        transfer_lots(
            tx,
            ctx,
            inventory_item_id,
            from_location_id,
            to_location_id,
            quantity,
            tracking.strategy,
        )
        .await?
    } else {
        Vec::new()
    };

    sqlx::query(
        "insert into inventory_level (id, scope, inventory_item_id, location_id, stocked_quantity)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, inventory_item_id, location_id)
         do update set stocked_quantity = inventory_level.stocked_quantity + $5",
    )
    .bind(InventoryLevelId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(to_location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    let id = StockTransferId::new();
    let transfer = sqlx::query_as::<_, StockTransfer>(&format!(
        "insert into stock_transfer
             (id, scope, inventory_item_id, from_location_id, to_location_id, quantity, reason)
         values ($1, $2, $3, $4, $5, $6, $7)
         returning {STOCK_TRANSFER_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(from_location_id.as_uuid())
    .bind(to_location_id.as_uuid())
    .bind(quantity)
    .bind(reason)
    .fetch_one(&mut **tx)
    .await?;

    for claim in &lots {
        sqlx::query(
            "insert into stock_transfer_lot
                 (id, scope, stock_transfer_id, inventory_lot_id, lot_code, expires_at, quantity)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(claim.inventory_lot_id)
        .bind(&claim.lot_code)
        .bind(claim.expires_at)
        .bind(claim.quantity)
        .execute(&mut **tx)
        .await?;
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "stock_transfer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "from_location_id": from_location_id,
                "to_location_id": to_location_id,
                "quantity": quantity,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.transferred",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "from_location_id": from_location_id,
                "to_location_id": to_location_id,
                "quantity": quantity,
            }),
        },
    )
    .await?;

    Ok(transfer)
}

pub async fn stock_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockTransferId,
) -> Result<StockTransfer> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, StockTransfer>(&format!(
        "select {STOCK_TRANSFER_COLUMNS} from stock_transfer where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("stock transfer"))
}

/// Every transfer of one item, newest cursor last.
pub async fn transfers_for_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    paging: Paging,
) -> Result<Page<StockTransfer>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, StockTransfer>(&format!(
        "select {STOCK_TRANSFER_COLUMNS}
         from stock_transfer
         where scope = $1
           and inventory_item_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
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

/// Promises stock without moving it.
///
/// The whole of the check is the `where` clause: one statement, so the row lock
/// serialises two callers reaching for the same last unit and the second sees
/// what the first wrote. Reading the level first and updating after would let
/// both pass a test that was true for neither by the time it was acted on.
///
/// `line_item_id` names a cart line — a reservation is always taken before
/// checkout writes the order that [`rebind_reservations`] later moves it onto.
#[allow(clippy::too_many_arguments)]
pub async fn reserve(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
    line_item_id: Option<LineItemId>,
    allows_backorder: bool,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Reservation> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let quantity = positive(quantity, "a reserved quantity")?;
    let tracking = tracking(tx, ctx, inventory_item_id).await?;

    let claimed = sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity + $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and (available_quantity >= $4 or $5)",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .bind(allows_backorder)
    .execute(&mut **tx)
    .await?;

    if claimed.rows_affected() == 0 {
        return Err(level_missing_or(
            tx,
            ctx,
            inventory_item_id,
            location_id,
            Error::out_of_stock_for(inventory_item_id.as_uuid()),
        )
        .await);
    }

    let id = ReservationId::new();
    let reservation = sqlx::query_as::<_, Reservation>(&format!(
        "insert into reservation_item
             (id, scope, inventory_item_id, location_id, quantity, cart_line_item_id,
              allows_backorder, expires_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning {RESERVATION_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .bind(line_item_id.map(|id| id.as_uuid()))
    .bind(allows_backorder)
    .bind(expires_at)
    .fetch_one(&mut **tx)
    .await?;

    if tracking.mode.by_lot() {
        let taken = allocate_lots(
            tx,
            ctx,
            id,
            inventory_item_id,
            location_id,
            quantity,
            tracking.strategy,
        )
        .await?;

        if taken < quantity && !allows_backorder {
            return Err(Error::out_of_stock_for(inventory_item_id.as_uuid()));
        }
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "reservation_item",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "quantity": quantity,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.reserved",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "quantity": quantity,
                "line_item_id": line_item_id,
            }),
        },
    )
    .await?;

    Ok(reservation)
}

/// Gives a promise back. `stocked` never moves: nothing left the shelf.
pub async fn release(tx: &mut Tx<'_>, ctx: &Ctx<'_>, reservation_id: ReservationId) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(reservation_id.as_uuid()),
        },
    )?;

    // Read before the delete: `reservation_lot` goes with the reservation.
    let claims = lot_claims(tx, ctx, reservation_id).await?;
    let reservation = take_reservation(tx, ctx, reservation_id).await?;
    unreserve(tx, ctx, &reservation).await?;
    unreserve_lots(tx, ctx, &claims).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "reservation_item",
            entity_id: reservation_id.as_uuid(),
            summary: serde_json::json!({ "quantity": reservation.quantity }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.released",
            entity_id: reservation_id.as_uuid(),
            payload: released_payload(&reservation),
        },
    )
    .await?;

    Ok(())
}

/// The stock actually leaves: the promise goes and `stocked` comes down with
/// it, in one transaction, because a shipment that only did half of this is
/// a count nobody can reconcile.
pub async fn fulfil(tx: &mut Tx<'_>, ctx: &Ctx<'_>, reservation_id: ReservationId) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(reservation_id.as_uuid()),
        },
    )?;

    let claims = lot_claims(tx, ctx, reservation_id).await?;
    let reservation = take_reservation(tx, ctx, reservation_id).await?;

    let moved = sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity - $4,
             stocked_quantity = stocked_quantity - $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and stocked_quantity >= $4",
    )
    .bind(ctx.scope.0)
    .bind(reservation.inventory_item_id.as_uuid())
    .bind(reservation.location_id.as_uuid())
    .bind(reservation.quantity)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(
            "that reservation is backordered and cannot ship yet",
        ));
    }

    for claim in &claims {
        ship_lot(tx, ctx, claim.inventory_lot_id, claim.quantity, true).await?;
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "reservation_item",
            entity_id: reservation_id.as_uuid(),
            summary: serde_json::json!({ "quantity": reservation.quantity, "fulfilled": true }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.fulfilled",
            entity_id: reservation_id.as_uuid(),
            payload: released_payload(&reservation),
        },
    )
    .await?;

    Ok(())
}

/// Every hold taken for one line, oldest first. Private on purpose: a line
/// holds as many reservations as the variant has inventory items, which is a
/// handful, not a page.
async fn line_reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
) -> Result<Vec<Reservation>> {
    Ok(sqlx::query_as::<_, Reservation>(&format!(
        "select {RESERVATION_COLUMNS} from reservation_item
         where scope = $1 and (cart_line_item_id = $2 or order_line_item_id = $2)
         order by created_at, id"
    ))
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?)
}

/// Moves a line's holds onto another line without touching a level.
///
/// A checkout reserves against the cart's line and then writes an order with
/// lines of its own; without this the stock an order holds is unreachable from
/// the order, which is how a cancellation came to give nothing back.
pub async fn rebind_reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    from: LineItemId,
    to: LineItemId,
) -> Result<u64> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    let moved = sqlx::query(
        "update reservation_item set order_line_item_id = $3, cart_line_item_id = null
         where scope = $1 and cart_line_item_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(from.as_uuid())
    .bind(to.as_uuid())
    .execute(&mut **tx)
    .await?;

    Ok(moved.rows_affected())
}

/// Gives back every hold every line of a cart has. What `cart::expire` calls
/// before it deletes an abandoned cart, so the reservation goes with an
/// audit row and a `stock.released` event rather than with nothing at all.
pub async fn release_cart(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: crate::id::CartId,
) -> Result<u64> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    let lines: Vec<uuid::Uuid> =
        sqlx::query_scalar("select id from cart_line_item where scope = $1 and cart_id = $2")
            .bind(ctx.scope.0)
            .bind(cart_id.as_uuid())
            .fetch_all(&mut **tx)
            .await?;

    let mut released = 0;
    for line in lines {
        released += release_line(tx, ctx, LineItemId::from_uuid(line)).await?;
    }
    Ok(released)
}

/// Gives back every hold a line has. Nothing left the shelf, so `stocked`
/// does not move.
pub async fn release_line(tx: &mut Tx<'_>, ctx: &Ctx<'_>, line_item_id: LineItemId) -> Result<u64> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    let held = line_reservations(tx, ctx, line_item_id).await?;
    for reservation in &held {
        let claims = lot_claims(tx, ctx, reservation.id).await?;
        take_reservation(tx, ctx, reservation.id).await?;
        unreserve(tx, ctx, reservation).await?;
        unreserve_lots(tx, ctx, &claims).await?;

        ctx.audit(
            tx,
            AuditEntry {
                actor: ctx.actor.clone(),
                action: Action::Delete,
                entity: "reservation_item",
                entity_id: reservation.id.as_uuid(),
                summary: serde_json::json!({ "quantity": reservation.quantity }),
            },
        )
        .await?;

        ctx.emit(
            tx,
            Event {
                name: "stock.released",
                entity_id: reservation.id.as_uuid(),
                payload: released_payload(reservation),
            },
        )
        .await?;
    }

    Ok(held.len() as u64)
}

/// Re-sizes what a line holds when the line's quantity changes.
///
/// Each hold is scaled by the same ratio it was taken at, so a bundle keeps
/// its proportions. A raise is a conditional update on the level: if the stock
/// is not there and the hold does not allow a backorder, the whole edit fails
/// rather than writing an order the shop cannot ship.
pub async fn rescale_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    from: i32,
    to: i32,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    if from <= 0 {
        return Err(Error::invalid("a line held stock for no quantity"));
    }
    if to < 0 {
        return Err(Error::invalid("a line cannot be taken below nothing"));
    }
    if from == to {
        return Ok(());
    }
    if to == 0 {
        release_line(tx, ctx, line_item_id).await?;
        return Ok(());
    }

    for reservation in line_reservations(tx, ctx, line_item_id).await? {
        let per_unit = reservation.quantity / from;
        let target = per_unit * to;
        let delta = target - reservation.quantity;
        if delta == 0 {
            continue;
        }

        let moved = sqlx::query(
            "update inventory_level
             set reserved_quantity = greatest(reserved_quantity + $4, 0)
             where scope = $1
               and inventory_item_id = $2
               and location_id = $3
               and ($4 <= 0 or available_quantity >= $4 or $5)",
        )
        .bind(ctx.scope.0)
        .bind(reservation.inventory_item_id.as_uuid())
        .bind(reservation.location_id.as_uuid())
        .bind(delta)
        .bind(reservation.allows_backorder)
        .execute(&mut **tx)
        .await?;

        if moved.rows_affected() == 0 {
            return Err(level_missing_or(
                tx,
                ctx,
                reservation.inventory_item_id,
                reservation.location_id,
                Error::out_of_stock_for(reservation.inventory_item_id.as_uuid()),
            )
            .await);
        }

        sqlx::query("update reservation_item set quantity = $3 where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(reservation.id.as_uuid())
            .bind(target)
            .execute(&mut **tx)
            .await?;

        // Given back and taken again rather than scaled in place: which lots a
        // larger hold reaches is a different answer, and a smaller one has to
        // give back the lot it took from rather than an average of them.
        let tracking = tracking(tx, ctx, reservation.inventory_item_id).await?;
        if tracking.mode.by_lot() {
            let claims = lot_claims(tx, ctx, reservation.id).await?;
            unreserve_lots(tx, ctx, &claims).await?;
            drop_lot_claims(tx, ctx, reservation.id).await?;

            let taken = allocate_lots(
                tx,
                ctx,
                reservation.id,
                reservation.inventory_item_id,
                reservation.location_id,
                target,
                tracking.strategy,
            )
            .await?;

            if taken < target && !reservation.allows_backorder {
                return Err(Error::out_of_stock_for(
                    reservation.inventory_item_id.as_uuid(),
                ));
            }
        }
    }

    ctx.emit(
        tx,
        Event {
            name: "stock.rescaled",
            entity_id: line_item_id.as_uuid(),
            payload: serde_json::json!({
                "line_item_id": line_item_id,
                "from": from,
                "to": to,
            }),
        },
    )
    .await?;

    Ok(())
}

/// The stock a line held actually leaves, for the part of it in one parcel.
///
/// One conditional update carries both halves: the hold comes down and
/// `stocked` with it. A line nothing was reserved for — a draft order typed in
/// the back office — still lowers `stocked`, because the goods left either way.
///
/// `fulfillment_item_id` is what the lots leaving are written against, and it
/// is the row a recall is answered from. `None` where there is no parcel.
#[allow(clippy::too_many_arguments)]
pub async fn fulfil_units(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
    fulfillment_item_id: Option<uuid::Uuid>,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let quantity = positive(quantity, "a fulfilled quantity")?;

    // Before the hold is taken down: `reservation_lot` hangs off the
    // reservation and goes with it, so which lots this line was holding is
    // only knowable while the hold is still there.
    ship_from_lots(
        tx,
        ctx,
        line_item_id,
        inventory_item_id,
        location_id,
        quantity,
        fulfillment_item_id,
    )
    .await?;

    let held = take_held(
        tx,
        ctx,
        line_item_id,
        inventory_item_id,
        location_id,
        quantity,
    )
    .await?;

    let moved = sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity - case when $5 then $4 else 0 end,
             stocked_quantity = stocked_quantity - $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and stocked_quantity >= $4
           and (not $5 or reserved_quantity >= $4)",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .bind(held)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(level_missing_or(
            tx,
            ctx,
            inventory_item_id,
            location_id,
            Error::conflict("there is not that much on the shelf to ship"),
        )
        .await);
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "reservation_item",
            entity_id: line_item_id.as_uuid(),
            summary: serde_json::json!({ "quantity": quantity, "fulfilled": true }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.fulfilled",
            entity_id: line_item_id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "quantity": quantity,
                "line_item_id": line_item_id,
            }),
        },
    )
    .await?;

    Ok(())
}

/// The goods come back before they left: `stocked` goes up and the line holds
/// them again, which is what a cancelled parcel means.
#[allow(clippy::too_many_arguments)]
pub async fn unfulfil_units(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
    fulfillment_item_id: Option<uuid::Uuid>,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let quantity = positive(quantity, "an unfulfilled quantity")?;

    let moved = sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity + $4,
             stocked_quantity = stocked_quantity + $4
         where scope = $1 and inventory_item_id = $2 and location_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::not_found("inventory level"));
    }

    let grown = sqlx::query(
        "update reservation_item set quantity = quantity + $5
         where scope = $1 and id = (
             select id from reservation_item
             where scope = $1
               and order_line_item_id = $2
               and inventory_item_id = $3
               and location_id = $4
             order by created_at, id
             limit 1
         )",
    )
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    if grown.rows_affected() == 0 {
        sqlx::query(
            "insert into reservation_item
                 (id, scope, inventory_item_id, location_id, quantity, order_line_item_id)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(ReservationId::new().as_uuid())
        .bind(ctx.scope.0)
        .bind(inventory_item_id.as_uuid())
        .bind(location_id.as_uuid())
        .bind(quantity)
        .bind(line_item_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    return_lots(tx, ctx, fulfillment_item_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "reservation_item",
            entity_id: line_item_id.as_uuid(),
            summary: serde_json::json!({ "quantity": quantity, "fulfilled": false }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.unfulfilled",
            entity_id: line_item_id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "quantity": quantity,
                "line_item_id": line_item_id,
            }),
        },
    )
    .await?;

    Ok(())
}

/// Takes `quantity` off the line's hold for one item, and says whether there
/// was a hold to take it off. Two statements rather than one because the
/// quantity check constraint forbids a reservation of nothing.
async fn take_held(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
) -> Result<bool> {
    let emptied = sqlx::query(
        "delete from reservation_item
         where scope = $1 and id = (
             select id from reservation_item
             where scope = $1
               and order_line_item_id = $2
               and inventory_item_id = $3
               and location_id = $4
               and quantity = $5
             order by created_at, id
             limit 1
         )",
    )
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    if emptied.rows_affected() > 0 {
        return Ok(true);
    }

    let lowered = sqlx::query(
        "update reservation_item set quantity = quantity - $5
         where scope = $1 and id = (
             select id from reservation_item
             where scope = $1
               and order_line_item_id = $2
               and inventory_item_id = $3
               and location_id = $4
               and quantity > $5
             order by created_at, id
             limit 1
         )",
    )
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    Ok(lowered.rows_affected() > 0)
}

/// Gives back everything whose hold has run out. A host runs this on a
/// schedule; a cart abandoned mid-checkout is what it is for.
pub async fn expire_reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<usize> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    // The lot claims go with the reservations they hang off, so they are read
    // before the delete rather than after it.
    let claims = sqlx::query_as::<_, LotClaim>(
        "select rl.inventory_lot_id, rl.quantity
         from reservation_lot rl
         join reservation_item ri on ri.scope = rl.scope and ri.id = rl.reservation_item_id
         where rl.scope = $1 and ri.expires_at is not null and ri.expires_at <= $2",
    )
    .bind(ctx.scope.0)
    .bind(now)
    .fetch_all(&mut **tx)
    .await?;

    let expired = sqlx::query_as::<_, Reservation>(&format!(
        "delete from reservation_item
         where scope = $1 and expires_at is not null and expires_at <= $2
         returning {RESERVATION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(now)
    .fetch_all(&mut **tx)
    .await?;

    unreserve_lots(tx, ctx, &claims).await?;

    for reservation in &expired {
        unreserve(tx, ctx, reservation).await?;

        ctx.audit(
            tx,
            AuditEntry {
                actor: ctx.actor.clone(),
                action: Action::Delete,
                entity: "reservation_item",
                entity_id: reservation.id.as_uuid(),
                summary: serde_json::json!({ "quantity": reservation.quantity }),
            },
        )
        .await?;

        ctx.emit(
            tx,
            Event {
                name: "stock.released",
                entity_id: reservation.id.as_uuid(),
                payload: released_payload(reservation),
            },
        )
        .await?;
    }

    Ok(expired.len())
}

pub async fn reservations_for_line_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    paging: Paging,
) -> Result<Page<Reservation>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, Reservation>(&format!(
        "select {RESERVATION_COLUMNS}
         from reservation_item
         where scope = $1 and (cart_line_item_id = $2 or order_line_item_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
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

pub async fn reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<Reservation>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, Reservation>(&format!(
        "select {RESERVATION_COLUMNS}
         from reservation_item
         where scope = $1
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4"
    ))
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

/// How many of a variant could still be sold, counting the bundle: whichever of
/// its items runs out first is the answer.
///
/// A variant with nothing linked to it is not stocked here and answers none;
/// whether it is nonetheless sellable is the catalogue's question, not this
/// module's.
pub async fn availability_for_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    location_id: Option<StockLocationId>,
) -> Result<i64> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let available: i64 = sqlx::query_scalar(
        "select coalesce(
                    min(floor(held.total::numeric / link.required_quantity)),
                    0
                )::bigint
         from variant_inventory_item link
         join lateral (
             select coalesce(sum(lvl.available_quantity), 0) as total
             from inventory_level lvl
             where lvl.scope = $1
               and lvl.inventory_item_id = link.inventory_item_id
               and ($3::uuid is null or lvl.location_id = $3)
         ) held on true
         where link.scope = $1 and link.variant_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(location_id.map(|id| id.as_uuid()))
    .fetch_one(&mut **tx)
    .await?;

    Ok(available.max(0))
}

async fn take_reservation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    reservation_id: ReservationId,
) -> Result<Reservation> {
    sqlx::query_as::<_, Reservation>(&format!(
        "delete from reservation_item
         where scope = $1 and id = $2
         returning {RESERVATION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(reservation_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("reservation"))
}

async fn unreserve(tx: &mut Tx<'_>, ctx: &Ctx<'_>, reservation: &Reservation) -> Result<()> {
    sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity - $4
         where scope = $1 and inventory_item_id = $2 and location_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(reservation.inventory_item_id.as_uuid())
    .bind(reservation.location_id.as_uuid())
    .bind(reservation.quantity)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn released_payload(reservation: &Reservation) -> serde_json::Value {
    serde_json::json!({
        "inventory_item_id": reservation.inventory_item_id,
        "location_id": reservation.location_id,
        "quantity": reservation.quantity,
        "line_item_id": reservation.line_item_id,
    })
}

/// Only ever on the failure path, so the happy one stays a single statement.
async fn level_missing_or(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    otherwise: Error,
) -> Error {
    let found: std::result::Result<Option<i32>, _> = sqlx::query_scalar(
        "select 1 from inventory_level
         where scope = $1 and inventory_item_id = $2 and location_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await;

    match found {
        Ok(None) => Error::not_found("inventory level"),
        Ok(Some(_)) => otherwise,
        Err(err) => Error::from(err),
    }
}

/// Only ever on the failure path, the way [`level_missing_or`] is: it tells a
/// row that is not there apart from a row that refused, without the happy path
/// paying for a second statement.
async fn location_missing_or(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StockLocationId,
    otherwise: Error,
) -> Error {
    let found: std::result::Result<Option<i32>, _> =
        sqlx::query_scalar("select 1 from stock_location where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .fetch_optional(&mut **tx)
            .await;

    match found {
        Ok(None) => Error::not_found("stock location"),
        Ok(Some(_)) => otherwise,
        Err(err) => Error::from(err),
    }
}

// ---------------------------------------------------------------------------
// Lots, serials and expiry.
//
// A level stays the total and every query that reads one keeps answering what
// it always answered. Underneath it, an item that asks for it is counted as
// lots: which code, which expiry, which day it arrived. A serial is a lot of
// one — the allocation, the expiry rule, the recall query and the sum
// invariant are the same statement for both, and the only thing that differs
// is how many rows one physical unit gets.
// ---------------------------------------------------------------------------

const LOT_COLUMNS: &str = "id, inventory_item_id, location_id, lot_code, is_serial, expires_at,
     received_at, stocked_quantity, reserved_quantity, available_quantity, created_at";

/// How many lots one allocation will walk before giving up. A warehouse
/// filling one line out of more than this many batches has a problem no
/// query fixes, and an unbounded loop under contention is a lock held all day.
const MAX_LOTS_PER_ALLOCATION: i64 = 100;

/// How an item is counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackingMode {
    /// A number per location, which is what every item is until it is not.
    Quantity,
    /// Batches, each with a code and possibly a date.
    Lot,
    /// One row per physical unit, its code unique across every location.
    Serial,
}

impl TrackingMode {
    pub fn by_lot(self) -> bool {
        !matches!(self, TrackingMode::Quantity)
    }

    fn as_str(self) -> &'static str {
        match self {
            TrackingMode::Quantity => "quantity",
            TrackingMode::Lot => "lot",
            TrackingMode::Serial => "serial",
        }
    }

    fn read(text: &str) -> TrackingMode {
        match text {
            "lot" => TrackingMode::Lot,
            "serial" => TrackingMode::Serial,
            _ => TrackingMode::Quantity,
        }
    }
}

/// Which units leave first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllocationStrategy {
    /// First expired, first out: what a shop selling food or medicine wants.
    Fefo,
    /// First in, first out: what a shop selling parts with no date wants.
    Fifo,
}

impl AllocationStrategy {
    fn as_str(self) -> &'static str {
        match self {
            AllocationStrategy::Fefo => "fefo",
            AllocationStrategy::Fifo => "fifo",
        }
    }

    fn read(text: &str) -> AllocationStrategy {
        match text {
            "fifo" => AllocationStrategy::Fifo,
            _ => AllocationStrategy::Fefo,
        }
    }

    /// The order clause itself. `nulls last` is the whole of FEFO's opinion
    /// about a lot with no date: it is not urgent, so it waits.
    fn order_by(self, prefix: &str) -> String {
        match self {
            AllocationStrategy::Fefo => format!(
                "{prefix}expires_at asc nulls last, {prefix}received_at asc, {prefix}id asc"
            ),
            AllocationStrategy::Fifo => format!(
                "{prefix}received_at asc, {prefix}expires_at asc nulls last, {prefix}id asc"
            ),
        }
    }
}

/// A batch of one item at one location. `available` is `stocked - reserved`,
/// computed by the database, exactly as a level's is.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct InventoryLot {
    pub id: InventoryLotId,
    pub inventory_item_id: InventoryItemId,
    pub location_id: StockLocationId,
    pub lot_code: String,
    pub is_serial: bool,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub received_at: chrono::DateTime<chrono::Utc>,
    pub stocked_quantity: i32,
    pub reserved_quantity: i32,
    pub available_quantity: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLot {
    /// The batch number, or the serial when the item is tracked by serial.
    pub lot_code: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// When the goods arrived, which is what FIFO orders by. Defaults to now.
    pub received_at: Option<chrono::DateTime<chrono::Utc>>,
    pub quantity: i32,
    pub supplier_reference: Option<String>,
}

/// One lot in one parcel: the row a recall is answered from.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct LotShipment {
    pub id: uuid::Uuid,
    pub fulfillment_id: FulfillmentId,
    pub order_id: Option<OrderId>,
    pub order_item_id: Option<uuid::Uuid>,
    pub lot_code: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub quantity: i32,
    pub shipped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Copy)]
struct Tracking {
    mode: TrackingMode,
    strategy: AllocationStrategy,
}

#[derive(Debug, Clone, FromRow)]
struct LotClaim {
    inventory_lot_id: uuid::Uuid,
    quantity: i32,
}

#[derive(Debug, Clone, FromRow)]
struct LotCandidate {
    id: uuid::Uuid,
    available_quantity: i32,
    lot_code: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One lot's share of what left in a parcel.
#[derive(Debug)]
struct Shipped {
    lot_id: uuid::Uuid,
    quantity: i32,
    lot_code: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, FromRow)]
struct HeldLot {
    id: uuid::Uuid,
    inventory_lot_id: uuid::Uuid,
    quantity: i32,
    lot_code: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// How this item is counted, and in which order its lots leave.
///
/// An item nobody wrote a row for is `quantity`, which is what makes every
/// path below a no-op for the items that were here before lots existed.
async fn tracking(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: InventoryItemId) -> Result<Tracking> {
    let row: Option<(String, String)> = sqlx::query_as(
        "select tracking_mode, allocation_strategy from inventory_item
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(match row {
        Some((mode, strategy)) => Tracking {
            mode: TrackingMode::read(&mode),
            strategy: AllocationStrategy::read(&strategy),
        },
        None => Tracking {
            mode: TrackingMode::Quantity,
            strategy: AllocationStrategy::Fefo,
        },
    })
}

/// Says how an item is counted and which of its lots leaves first.
///
/// Turning tracking on for an item that already has stock does not split what
/// is on the shelf into lots — there is nobody who knows which lots those
/// were. Receive the lots and count the level down to meet them.
pub async fn set_tracking(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    mode: TrackingMode,
    strategy: AllocationStrategy,
) -> Result<InventoryItem> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let item = sqlx::query_as::<_, InventoryItem>(
        "update inventory_item
         set tracking_mode = $3, allocation_strategy = $4
         where scope = $1 and id = $2 and deleted_at is null
         returning id, sku, title, requires_shipping, created_at",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(mode.as_str())
    .bind(strategy.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("inventory item"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_item",
            entity_id: inventory_item_id.as_uuid(),
            summary: serde_json::json!({
                "tracking_mode": mode.as_str(),
                "allocation_strategy": strategy.as_str(),
            }),
        },
    )
    .await?;

    Ok(item)
}

/// Goods arrive as a batch: the lot goes up and the level goes up with it, so
/// the level stays the sum of its lots.
pub async fn receive_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    new: NewLot,
) -> Result<InventoryLot> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let quantity = positive(new.quantity, "a received quantity")?;
    let code = new.lot_code.trim();
    if code.is_empty() {
        return Err(Error::invalid("a lot needs a code"));
    }

    let tracking = tracking(tx, ctx, inventory_item_id).await?;
    if !tracking.mode.by_lot() {
        return Err(Error::invalid(
            "that item is counted as a number; turn lot tracking on first",
        ));
    }

    let is_serial = tracking.mode == TrackingMode::Serial;
    if is_serial && quantity != 1 {
        return Err(Error::invalid("a serial names one unit and only one"));
    }

    let (item_here, location_here): (bool, bool) = sqlx::query_as(
        "select exists (select 1 from inventory_item
                        where scope = $1 and id = $2 and deleted_at is null),
                exists (select 1 from stock_location where scope = $1 and id = $3)",
    )
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if !item_here {
        return Err(Error::not_found("inventory item"));
    }
    if !location_here {
        return Err(Error::not_found("stock location"));
    }

    // The lot before the level: a serial that turns out to have arrived once
    // already must leave the total exactly where it found it.
    // A serial arriving twice is two claims about one unit, so it is refused
    // rather than added up; a batch arriving twice is a second delivery.
    let statement = if is_serial {
        format!(
            "insert into inventory_lot
                 (id, scope, inventory_item_id, location_id, lot_code, is_serial,
                  expires_at, received_at, stocked_quantity, supplier_reference)
             values ($1, $2, $3, $4, $5, true, $6, $7, $8, $9)
             on conflict (scope, inventory_item_id, lot_code) where is_serial do nothing
             returning {LOT_COLUMNS}"
        )
    } else {
        format!(
            "insert into inventory_lot
                 (id, scope, inventory_item_id, location_id, lot_code, is_serial,
                  expires_at, received_at, stocked_quantity, supplier_reference)
             values ($1, $2, $3, $4, $5, false, $6, $7, $8, $9)
             on conflict (scope, inventory_item_id, location_id, lot_code)
             do update set stocked_quantity =
                               inventory_lot.stocked_quantity + excluded.stocked_quantity,
                           expires_at = coalesce(excluded.expires_at, inventory_lot.expires_at)
             returning {LOT_COLUMNS}"
        )
    };

    let lot = sqlx::query_as::<_, InventoryLot>(&statement)
        .bind(InventoryLotId::new().as_uuid())
        .bind(ctx.scope.0)
        .bind(inventory_item_id.as_uuid())
        .bind(location_id.as_uuid())
        .bind(code)
        .bind(new.expires_at)
        .bind(new.received_at.unwrap_or_else(|| ctx.now()))
        .bind(quantity)
        .bind(new.supplier_reference.as_deref())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::conflict("that serial has already been received"))?;

    sqlx::query(
        "insert into inventory_level
             (id, scope, inventory_item_id, location_id, stocked_quantity)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, inventory_item_id, location_id)
         do update set stocked_quantity = inventory_level.stocked_quantity + $5",
    )
    .bind(InventoryLevelId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_lot",
            entity_id: lot.id.as_uuid(),
            summary: serde_json::json!({
                "lot_code": lot.lot_code,
                "quantity": quantity,
                "expires_at": lot.expires_at,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.lot_received",
            entity_id: lot.id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": inventory_item_id,
                "location_id": location_id,
                "lot_code": lot.lot_code,
                "quantity": quantity,
                "expires_at": lot.expires_at,
            }),
        },
    )
    .await?;

    Ok(lot)
}

/// A breakage, a recount or a write-off against one lot. The level moves with
/// it, because the level is the sum of the lots and nothing else.
pub async fn adjust_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lot_id: InventoryLotId,
    delta: i32,
    reason: Option<&str>,
) -> Result<InventoryLot> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(lot_id.as_uuid()),
        },
    )?;

    if delta == 0 {
        return Err(Error::invalid("an adjustment of none changes nothing"));
    }

    let lot = sqlx::query_as::<_, InventoryLot>(&format!(
        "update inventory_lot
         set stocked_quantity = stocked_quantity + $3
         where scope = $1 and id = $2 and stocked_quantity + $3 >= 0
         returning {LOT_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(lot_id.as_uuid())
    .bind(delta)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(lot) = lot else {
        return Err(lot_missing_or(
            tx,
            ctx,
            lot_id,
            Error::conflict("that would leave less than none in the lot"),
        )
        .await);
    };

    let moved = sqlx::query(
        "update inventory_level
         set stocked_quantity = stocked_quantity + $4
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and stocked_quantity + $4 >= 0",
    )
    .bind(ctx.scope.0)
    .bind(lot.inventory_item_id.as_uuid())
    .bind(lot.location_id.as_uuid())
    .bind(delta)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(
            "the level cannot follow that lot below none",
        ));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "inventory_lot",
            entity_id: lot.id.as_uuid(),
            summary: serde_json::json!({
                "delta": delta,
                "reason": reason,
                "stocked_quantity": lot.stocked_quantity,
            }),
        },
    )
    .await?;

    Ok(lot)
}

pub async fn lot(tx: &mut Tx<'_>, ctx: &Ctx<'_>, lot_id: InventoryLotId) -> Result<InventoryLot> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(lot_id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, InventoryLot>(&format!(
        "select {LOT_COLUMNS} from inventory_lot where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(lot_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("inventory lot"))
}

/// Every lot of one item, newest cursor last. `location_id` narrows it to one
/// shelf; `None` is every shelf the item is on.
pub async fn lots_for_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    location_id: Option<StockLocationId>,
    paging: Paging,
) -> Result<Page<InventoryLot>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(inventory_item_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, InventoryLot>(&format!(
        "select {LOT_COLUMNS}
         from inventory_lot
         where scope = $1
           and inventory_item_id = $2
           and ($3::uuid is null or location_id = $3)
           and ($4::timestamptz is null or (created_at, id) > ($4, $5))
         order by created_at, id
         limit $6"
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.map(|id| id.as_uuid()))
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

/// What is about to go out of date, so a shop can discount it rather than
/// write it off. Lots with nothing left in them are not ageing stock.
pub async fn expiring_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    before: chrono::DateTime<chrono::Utc>,
    paging: Paging,
) -> Result<Page<InventoryLot>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, InventoryLot>(&format!(
        "select {LOT_COLUMNS}
         from inventory_lot
         where scope = $1
           and expires_at is not null
           and expires_at <= $2
           and stocked_quantity > 0
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(before)
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

/// The recall answer: which parcels this lot went out in, and whose orders
/// they were.
///
/// `order_id` is null for a parcel raised against no order at all, which is
/// the only case where the lot went somewhere this cannot name.
pub async fn orders_for_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lot_id: InventoryLotId,
    paging: Paging,
) -> Result<Page<LotShipment>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Inventory {
            id: Some(lot_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, LotShipment>(
        "select fl.id,
                fi.fulfillment_id,
                oi.order_id,
                fi.line_item_id as order_item_id,
                fl.lot_code,
                fl.expires_at,
                fl.quantity,
                f.shipped_at,
                f.delivered_at,
                f.canceled_at,
                fl.created_at
         from fulfillment_lot fl
         join fulfillment_item fi on fi.scope = fl.scope and fi.id = fl.fulfillment_item_id
         join fulfillment f on f.scope = fl.scope and f.id = fi.fulfillment_id
         left join order_item oi on oi.scope = fl.scope and oi.id = fi.line_item_id
         where fl.scope = $1
           and fl.inventory_lot_id = $2
           and ($3::timestamptz is null or (fl.created_at, fl.id) > ($3, $4))
         order by fl.created_at, fl.id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(lot_id.as_uuid())
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id,
    }))
}

/// Promises one named lot rather than whichever the strategy would pick: the
/// third thing a real warehouse asks for, after FEFO and FIFO.
///
/// `line_item_id`, like [`reserve`]'s, names a cart line.
pub async fn reserve_from_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lot_id: InventoryLotId,
    quantity: i32,
    line_item_id: Option<LineItemId>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Reservation> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Inventory {
            id: Some(lot_id.as_uuid()),
        },
    )?;

    let quantity = positive(quantity, "a reserved quantity")?;
    let now = ctx.now();

    let held = sqlx::query_as::<_, InventoryLot>(&format!(
        "update inventory_lot
         set reserved_quantity = reserved_quantity + $3
         where scope = $1
           and id = $2
           and available_quantity >= $3
           and (expires_at is null or expires_at > $4)
         returning {LOT_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(lot_id.as_uuid())
    .bind(quantity)
    .bind(now)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(held) = held else {
        return Err(lot_missing_or(
            tx,
            ctx,
            lot_id,
            Error::conflict("that lot has no such quantity left, or has expired"),
        )
        .await);
    };

    let raised = sqlx::query(
        "update inventory_level
         set reserved_quantity = reserved_quantity + $4
         where scope = $1 and inventory_item_id = $2 and location_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(held.inventory_item_id.as_uuid())
    .bind(held.location_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    if raised.rows_affected() == 0 {
        return Err(Error::not_found("inventory level"));
    }

    let id = ReservationId::new();
    let reservation = sqlx::query_as::<_, Reservation>(&format!(
        "insert into reservation_item
             (id, scope, inventory_item_id, location_id, quantity, cart_line_item_id,
              allows_backorder, expires_at)
         values ($1, $2, $3, $4, $5, $6, false, $7)
         returning {RESERVATION_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(held.inventory_item_id.as_uuid())
    .bind(held.location_id.as_uuid())
    .bind(quantity)
    .bind(line_item_id.map(|id| id.as_uuid()))
    .bind(expires_at)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        "insert into reservation_lot (id, scope, reservation_item_id, inventory_lot_id, quantity)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(lot_id.as_uuid())
    .bind(quantity)
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "reservation_item",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "lot_code": held.lot_code, "quantity": quantity }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "stock.reserved",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "inventory_item_id": held.inventory_item_id,
                "location_id": held.location_id,
                "quantity": quantity,
                "line_item_id": line_item_id,
                "lot_code": held.lot_code,
            }),
        },
    )
    .await?;

    Ok(reservation)
}

/// Claims `quantity` out of an item's lots in the strategy's order, and says
/// how much it managed to claim.
///
/// The candidates are read first and then each is claimed by a conditional
/// update, which is the only order that survives two callers: the read may be
/// stale by the time the update runs, and the update is what notices — a lot
/// somebody else emptied fails its own `where` and the loop moves on to the
/// next one rather than overselling it.
async fn allocate_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    reservation_id: ReservationId,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
    strategy: AllocationStrategy,
) -> Result<i32> {
    let now = ctx.now();

    let candidates = sqlx::query_as::<_, LotCandidate>(&format!(
        "select id, available_quantity, lot_code, expires_at
         from inventory_lot
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and available_quantity > 0
           and (expires_at is null or expires_at > $4)
         order by {}
         limit $5",
        strategy.order_by("")
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(now)
    .bind(MAX_LOTS_PER_ALLOCATION)
    .fetch_all(&mut **tx)
    .await?;

    let mut remaining = quantity;
    for candidate in candidates {
        if remaining <= 0 {
            break;
        }
        let take = remaining.min(candidate.available_quantity);
        if take <= 0 {
            continue;
        }

        let claimed = sqlx::query(
            "update inventory_lot
             set reserved_quantity = reserved_quantity + $3
             where scope = $1
               and id = $2
               and available_quantity >= $3
               and (expires_at is null or expires_at > $4)",
        )
        .bind(ctx.scope.0)
        .bind(candidate.id)
        .bind(take)
        .bind(now)
        .execute(&mut **tx)
        .await?;

        if claimed.rows_affected() == 0 {
            continue;
        }

        sqlx::query(
            "insert into reservation_lot
                 (id, scope, reservation_item_id, inventory_lot_id, quantity)
             values ($1, $2, $3, $4, $5)
             on conflict do nothing",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(reservation_id.as_uuid())
        .bind(candidate.id)
        .bind(take)
        .execute(&mut **tx)
        .await?;

        remaining -= take;
    }

    Ok(quantity - remaining)
}

/// What lots a reservation is holding. Bounded by what an allocation could
/// have written, which is why it is private and takes no `Paging`.
async fn lot_claims(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    reservation_id: ReservationId,
) -> Result<Vec<LotClaim>> {
    Ok(sqlx::query_as::<_, LotClaim>(
        "select inventory_lot_id, quantity from reservation_lot
         where scope = $1 and reservation_item_id = $2
         order by id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(reservation_id.as_uuid())
    .bind(MAX_LOTS_PER_ALLOCATION)
    .fetch_all(&mut **tx)
    .await?)
}

/// Gives claimed quantity back to the lots it came from. Nothing left the
/// shelf, so `stocked` does not move.
async fn unreserve_lots(tx: &mut Tx<'_>, ctx: &Ctx<'_>, claims: &[LotClaim]) -> Result<()> {
    for claim in claims {
        sqlx::query(
            "update inventory_lot
             set reserved_quantity = greatest(reserved_quantity - $3, 0)
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(claim.inventory_lot_id)
        .bind(claim.quantity)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn drop_lot_claims(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    reservation_id: ReservationId,
) -> Result<()> {
    sqlx::query("delete from reservation_lot where scope = $1 and reservation_item_id = $2")
        .bind(ctx.scope.0)
        .bind(reservation_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// One lot's goods leave. `held` says whether a reservation is coming down
/// with them; a draft order never had one.
async fn ship_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lot_id: uuid::Uuid,
    quantity: i32,
    held: bool,
) -> Result<()> {
    let moved = sqlx::query(
        "update inventory_lot
         set stocked_quantity = stocked_quantity - $3,
             reserved_quantity = reserved_quantity - case when $4 then $3 else 0 end
         where scope = $1
           and id = $2
           and stocked_quantity >= $3
           and (not $4 or reserved_quantity >= $3)",
    )
    .bind(ctx.scope.0)
    .bind(lot_id)
    .bind(quantity)
    .bind(held)
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(
            "that lot has not got that much left to ship",
        ));
    }

    Ok(())
}

/// Takes the units for one parcel out of the lots the line was holding, then
/// out of whatever else is on the shelf, and writes down which lots went.
///
/// The order is the strategy's: a parcel packed out of a hold still ships the
/// earliest-expiring of the lots that hold was spread across.
async fn ship_from_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    line_item_id: LineItemId,
    inventory_item_id: InventoryItemId,
    location_id: StockLocationId,
    quantity: i32,
    fulfillment_item_id: Option<uuid::Uuid>,
) -> Result<()> {
    let tracking = tracking(tx, ctx, inventory_item_id).await?;
    if !tracking.mode.by_lot() {
        return Ok(());
    }

    let mut remaining = quantity;
    let mut shipped: Vec<Shipped> = Vec::new();

    let claims = sqlx::query_as::<_, HeldLot>(&format!(
        "select rl.id, rl.inventory_lot_id, rl.quantity, l.lot_code, l.expires_at
         from reservation_lot rl
         join reservation_item ri on ri.scope = rl.scope and ri.id = rl.reservation_item_id
         join inventory_lot l on l.scope = rl.scope and l.id = rl.inventory_lot_id
         where rl.scope = $1
           and ri.order_line_item_id = $2
           and ri.inventory_item_id = $3
           and ri.location_id = $4
         order by {}
         limit $5",
        tracking.strategy.order_by("l.")
    ))
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(location_id.as_uuid())
    .bind(MAX_LOTS_PER_ALLOCATION)
    .fetch_all(&mut **tx)
    .await?;

    for claim in claims {
        if remaining <= 0 {
            break;
        }
        let take = remaining.min(claim.quantity);
        ship_lot(tx, ctx, claim.inventory_lot_id, take, true).await?;

        if take == claim.quantity {
            sqlx::query("delete from reservation_lot where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(claim.id)
                .execute(&mut **tx)
                .await?;
        } else {
            sqlx::query(
                "update reservation_lot set quantity = quantity - $3
                 where scope = $1 and id = $2 and quantity > $3",
            )
            .bind(ctx.scope.0)
            .bind(claim.id)
            .bind(take)
            .execute(&mut **tx)
            .await?;
        }

        shipped.push(Shipped {
            lot_id: claim.inventory_lot_id,
            quantity: take,
            lot_code: claim.lot_code,
            expires_at: claim.expires_at,
        });
        remaining -= take;
    }

    if remaining > 0 {
        let candidates = sqlx::query_as::<_, LotCandidate>(&format!(
            "select id, available_quantity, lot_code, expires_at
             from inventory_lot
             where scope = $1
               and inventory_item_id = $2
               and location_id = $3
               and available_quantity > 0
             order by {}
             limit $4",
            tracking.strategy.order_by("")
        ))
        .bind(ctx.scope.0)
        .bind(inventory_item_id.as_uuid())
        .bind(location_id.as_uuid())
        .bind(MAX_LOTS_PER_ALLOCATION)
        .fetch_all(&mut **tx)
        .await?;

        for candidate in candidates {
            if remaining <= 0 {
                break;
            }
            let take = remaining.min(candidate.available_quantity);
            if take <= 0 {
                continue;
            }

            let moved = sqlx::query(
                "update inventory_lot
                 set stocked_quantity = stocked_quantity - $3
                 where scope = $1 and id = $2 and available_quantity >= $3",
            )
            .bind(ctx.scope.0)
            .bind(candidate.id)
            .bind(take)
            .execute(&mut **tx)
            .await?;

            if moved.rows_affected() == 0 {
                continue;
            }

            shipped.push(Shipped {
                lot_id: candidate.id,
                quantity: take,
                lot_code: candidate.lot_code,
                expires_at: candidate.expires_at,
            });
            remaining -= take;
        }
    }

    if remaining > 0 {
        return Err(Error::conflict("there is not that much in any lot to ship"));
    }

    let Some(fulfillment_item_id) = fulfillment_item_id else {
        return Ok(());
    };

    for went in shipped {
        sqlx::query(
            "insert into fulfillment_lot
                 (id, scope, fulfillment_item_id, inventory_lot_id, lot_code, expires_at, quantity)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(uuid::Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(fulfillment_item_id)
        .bind(went.lot_id)
        .bind(&went.lot_code)
        .bind(went.expires_at)
        .bind(went.quantity)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

#[derive(Debug, Clone, FromRow)]
struct TransferCandidate {
    id: uuid::Uuid,
    available_quantity: i32,
    lot_code: String,
    is_serial: bool,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One lot's share of a transfer, at the destination: what
/// [`transfer_stock`] writes into `stock_transfer_lot`.
struct LotTransferClaim {
    inventory_lot_id: uuid::Uuid,
    lot_code: String,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    quantity: i32,
}

/// Moves `quantity` of an item's lots from one location to another, in the
/// item's own allocation order, and says which lots the destination now has.
///
/// A serial's code is unique across every location already, so a serial does
/// not get shipped from one row and received into another — its one row is
/// relocated. A batch lot decrements at the source and merges into whatever
/// the destination already has under that code, the way [`receive_lot`] does
/// for a delivery.
async fn transfer_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    inventory_item_id: InventoryItemId,
    from_location_id: StockLocationId,
    to_location_id: StockLocationId,
    quantity: i32,
    strategy: AllocationStrategy,
) -> Result<Vec<LotTransferClaim>> {
    let candidates = sqlx::query_as::<_, TransferCandidate>(&format!(
        "select id, available_quantity, lot_code, is_serial, expires_at
         from inventory_lot
         where scope = $1
           and inventory_item_id = $2
           and location_id = $3
           and available_quantity > 0
         order by {}
         limit $4",
        strategy.order_by("")
    ))
    .bind(ctx.scope.0)
    .bind(inventory_item_id.as_uuid())
    .bind(from_location_id.as_uuid())
    .bind(MAX_LOTS_PER_ALLOCATION)
    .fetch_all(&mut **tx)
    .await?;

    let mut remaining = quantity;
    let mut claims = Vec::new();

    for candidate in candidates {
        if remaining <= 0 {
            break;
        }
        let take = remaining.min(candidate.available_quantity);
        if take <= 0 {
            continue;
        }

        if candidate.is_serial {
            let moved = sqlx::query_scalar::<_, uuid::Uuid>(
                "update inventory_lot
                 set location_id = $4
                 where scope = $1 and id = $2 and location_id = $3 and available_quantity >= $5
                 returning id",
            )
            .bind(ctx.scope.0)
            .bind(candidate.id)
            .bind(from_location_id.as_uuid())
            .bind(to_location_id.as_uuid())
            .bind(take)
            .fetch_optional(&mut **tx)
            .await?;

            let Some(lot_id) = moved else {
                continue;
            };

            claims.push(LotTransferClaim {
                inventory_lot_id: lot_id,
                lot_code: candidate.lot_code,
                expires_at: candidate.expires_at,
                quantity: take,
            });
            remaining -= take;
            continue;
        }

        let left = sqlx::query(
            "update inventory_lot
             set stocked_quantity = stocked_quantity - $3
             where scope = $1 and id = $2 and available_quantity >= $3",
        )
        .bind(ctx.scope.0)
        .bind(candidate.id)
        .bind(take)
        .execute(&mut **tx)
        .await?;

        if left.rows_affected() == 0 {
            continue;
        }

        let arrived: uuid::Uuid = sqlx::query_scalar(
            "insert into inventory_lot
                 (id, scope, inventory_item_id, location_id, lot_code, is_serial, expires_at,
                  stocked_quantity)
             values ($1, $2, $3, $4, $5, false, $6, $7)
             on conflict (scope, inventory_item_id, location_id, lot_code)
             do update set stocked_quantity =
                               inventory_lot.stocked_quantity + excluded.stocked_quantity,
                           expires_at = coalesce(inventory_lot.expires_at, excluded.expires_at)
             returning id",
        )
        .bind(InventoryLotId::new().as_uuid())
        .bind(ctx.scope.0)
        .bind(inventory_item_id.as_uuid())
        .bind(to_location_id.as_uuid())
        .bind(&candidate.lot_code)
        .bind(candidate.expires_at)
        .bind(take)
        .fetch_one(&mut **tx)
        .await?;

        claims.push(LotTransferClaim {
            inventory_lot_id: arrived,
            lot_code: candidate.lot_code,
            expires_at: candidate.expires_at,
            quantity: take,
        });
        remaining -= take;
    }

    if remaining > 0 {
        return Err(Error::conflict(
            "there is not that much in any lot to transfer",
        ));
    }

    Ok(claims)
}

/// A parcel cancelled before it left: the lots it named get their units back,
/// held again, and the record of what was in it goes with it.
async fn return_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    fulfillment_item_id: Option<uuid::Uuid>,
) -> Result<()> {
    let Some(fulfillment_item_id) = fulfillment_item_id else {
        return Ok(());
    };

    let went = sqlx::query_as::<_, LotClaim>(
        "delete from fulfillment_lot
         where scope = $1 and fulfillment_item_id = $2
         returning inventory_lot_id, quantity",
    )
    .bind(ctx.scope.0)
    .bind(fulfillment_item_id)
    .fetch_all(&mut **tx)
    .await?;

    for row in went {
        sqlx::query(
            "update inventory_lot
             set stocked_quantity = stocked_quantity + $3,
                 reserved_quantity = reserved_quantity + $3
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(row.inventory_lot_id)
        .bind(row.quantity)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

async fn lot_missing_or(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lot_id: InventoryLotId,
    otherwise: Error,
) -> Error {
    let found: std::result::Result<Option<i32>, _> =
        sqlx::query_scalar("select 1 from inventory_lot where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(lot_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await;

    match found {
        Ok(None) => Error::not_found("inventory lot"),
        Ok(Some(_)) => otherwise,
        Err(err) => Error::from(err),
    }
}
