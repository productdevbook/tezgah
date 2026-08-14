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

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{
    InventoryItemId, InventoryLevelId, LineItemId, ReservationId, SalesChannelId, StockLocationId,
    VariantId,
};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const LEVEL_COLUMNS: &str = "id, inventory_item_id, location_id, stocked_quantity,
     reserved_quantity, incoming_quantity, available_quantity, created_at";

const RESERVATION_COLUMNS: &str = "id, inventory_item_id, location_id, quantity, line_item_id,
     allows_backorder, expires_at, created_at";

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

fn as_conflict(err: sqlx::Error, what: &'static str) -> Error {
    match err.as_database_error() {
        Some(db) if db.is_foreign_key_violation() || db.is_unique_violation() => {
            Error::conflict(what)
        }
        _ => Error::from(err),
    }
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
    let location = sqlx::query_as::<_, StockLocation>(
        "insert into stock_location (id, scope, name)
         values ($1, $2, $3)
         returning id, name, address_id, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .fetch_one(&mut **tx)
    .await
    .map_err(|err| as_conflict(err, "a stock location by that name is already here"))?;

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
         returning id, name, address_id, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(name.trim())
    .fetch_optional(&mut **tx)
    .await
    .map_err(|err| as_conflict(err, "a stock location by that name is already here"))?
    .ok_or_else(|| Error::not_found("stock location"))?;

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

    let done = sqlx::query("delete from stock_location where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await
        .map_err(|err| as_conflict(err, "stock is still counted at that location"))?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("stock location"));
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
    .await
    .map_err(|err| as_conflict(err, "no such location or channel"))?;

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
         returning id, sku, title, requires_shipping, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.sku.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .bind(new.title.as_deref())
    .bind(new.requires_shipping)
    .fetch_one(&mut **tx)
    .await
    .map_err(|err| as_conflict(err, "that sku is already counted here"))?;

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
         values ($1, $2, $3, $4, $5)
         on conflict (scope, variant_id, inventory_item_id)
         do update set required_quantity = excluded.required_quantity
         returning variant_id, inventory_item_id, required_quantity",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(inventory_item_id.as_uuid())
    .bind(required_quantity)
    .fetch_one(&mut **tx)
    .await
    .map_err(|err| as_conflict(err, "no such variant or inventory item"))?;

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
         order by inventory_item_id",
    )
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
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

    let level = sqlx::query_as::<_, InventoryLevel>(&format!(
        "insert into inventory_level
             (id, scope, inventory_item_id, location_id, stocked_quantity, incoming_quantity)
         values ($1, $2, $3, $4, $5, $6)
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
    .fetch_one(&mut **tx)
    .await
    .map_err(|err| as_conflict(err, "no such inventory item or location"))?;

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

    Ok(level)
}

/// Promises stock without moving it.
///
/// The whole of the check is the `where` clause: one statement, so the row lock
/// serialises two callers reaching for the same last unit and the second sees
/// what the first wrote. Reading the level first and updating after would let
/// both pass a test that was true for neither by the time it was acted on.
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
             (id, scope, inventory_item_id, location_id, quantity, line_item_id,
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

    let reservation = take_reservation(tx, ctx, reservation_id).await?;
    unreserve(tx, ctx, &reservation).await?;

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

/// Gives back everything whose hold has run out. A host runs this on a
/// schedule; a cart abandoned mid-checkout is what it is for.
pub async fn expire_reservations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<usize> {
    let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;

    let expired = sqlx::query_as::<_, Reservation>(&format!(
        "delete from reservation_item
         where scope = $1 and expires_at is not null and expires_at <= $2
         returning {RESERVATION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(now)
    .fetch_all(&mut **tx)
    .await?;

    for reservation in &expired {
        unreserve(tx, ctx, reservation).await?;

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
) -> Result<Vec<Reservation>> {
    let _: Permit = ctx.permit(Action::View, Resource::Inventory { id: None })?;

    let rows = sqlx::query_as::<_, Reservation>(&format!(
        "select {RESERVATION_COLUMNS}
         from reservation_item
         where scope = $1 and line_item_id = $2
         order by created_at, id"
    ))
    .bind(ctx.scope.0)
    .bind(line_item_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
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
