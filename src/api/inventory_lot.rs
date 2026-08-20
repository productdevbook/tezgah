//! Lots, in the back office.
//!
//! An item is counted as a number until somebody says otherwise, and this is
//! where they say it. Everything below hangs off that one switch: without it
//! there are no lots to receive, nothing for FEFO to order, no expiry to watch
//! and nothing for a recall to answer with.
//!
//! There is no storefront half. A shopper buys the goods, not the batch, and
//! which batch they were given is the shop's record rather than theirs.

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::{
    FulfillmentId, InventoryItemId, InventoryLotId, LineItemId, OrderId, StockLocationId,
};
use crate::inventory;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};

use super::admin_catalogue::ReservationView;
use super::{Method, Route, Surface};

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

fn map<T, U>(page: Page<T>, into: impl Fn(T) -> U) -> Page<U> {
    Page {
        items: page.items.into_iter().map(into).collect(),
        next: page.next,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotView {
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
}

impl From<inventory::InventoryLot> for LotView {
    fn from(row: inventory::InventoryLot) -> Self {
        LotView {
            id: row.id,
            inventory_item_id: row.inventory_item_id,
            location_id: row.location_id,
            lot_code: row.lot_code,
            is_serial: row.is_serial,
            expires_at: row.expires_at,
            received_at: row.received_at,
            stocked_quantity: row.stocked_quantity,
            reserved_quantity: row.reserved_quantity,
            available_quantity: row.available_quantity,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedItemView {
    pub id: InventoryItemId,
    pub sku: Option<String>,
    pub title: Option<String>,
    pub tracking_mode: inventory::TrackingMode,
    pub allocation_strategy: inventory::AllocationStrategy,
}

/// One parcel a lot went out in: the recall answer, one row per shipment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotShipmentView {
    pub id: uuid::Uuid,
    pub fulfillment_id: FulfillmentId,
    pub order_id: Option<OrderId>,
    pub lot_code: String,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub quantity: i32,
    pub shipped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<inventory::LotShipment> for LotShipmentView {
    fn from(row: inventory::LotShipment) -> Self {
        LotShipmentView {
            id: row.id,
            fulfillment_id: row.fulfillment_id,
            order_id: row.order_id,
            lot_code: row.lot_code,
            expires_at: row.expires_at,
            quantity: row.quantity,
            shipped_at: row.shipped_at,
            delivered_at: row.delivered_at,
            canceled_at: row.canceled_at,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetTracking {
    pub tracking_mode: inventory::TrackingMode,
    pub allocation_strategy: inventory::AllocationStrategy,
}

pub async fn set_tracking(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    body: SetTracking,
) -> Result<TrackedItemView> {
    let item =
        inventory::set_tracking(tx, ctx, id, body.tracking_mode, body.allocation_strategy).await?;

    Ok(TrackedItemView {
        id: item.id,
        sku: item.sku,
        title: item.title,
        tracking_mode: body.tracking_mode,
        allocation_strategy: body.allocation_strategy,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiveLot {
    pub location_id: StockLocationId,
    pub lot_code: String,
    pub quantity: i32,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub received_at: Option<chrono::DateTime<chrono::Utc>>,
    pub supplier_reference: Option<String>,
}

pub async fn receive_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    body: ReceiveLot,
) -> Result<LotView> {
    let lot = inventory::receive_lot(
        tx,
        ctx,
        id,
        body.location_id,
        inventory::NewLot {
            lot_code: body.lot_code,
            expires_at: body.expires_at,
            received_at: body.received_at,
            quantity: body.quantity,
            supplier_reference: body.supplier_reference,
        },
    )
    .await?;

    Ok(LotView::from(lot))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustLot {
    /// Signed: a breakage is negative, a recount either way.
    pub delta: i32,
    pub reason: Option<String>,
}

pub async fn adjust_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryLotId,
    body: AdjustLot,
) -> Result<LotView> {
    Ok(LotView::from(
        inventory::adjust_lot(tx, ctx, id, body.delta, body.reason.as_deref()).await?,
    ))
}

pub async fn get_lot(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: InventoryLotId) -> Result<LotView> {
    Ok(LotView::from(inventory::lot(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListLots {
    pub location_id: Option<StockLocationId>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryItemId,
    query: ListLots,
) -> Result<Page<LotView>> {
    let page = inventory::lots_for_item(
        tx,
        ctx,
        id,
        query.location_id,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;

    Ok(map(page, LotView::from))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListExpiring {
    /// Everything going out of date before this moment.
    pub before: chrono::DateTime<chrono::Utc>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_expiring_lots(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListExpiring,
) -> Result<Page<LotView>> {
    let page = inventory::expiring_lots(
        tx,
        ctx,
        query.before,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;

    Ok(map(page, LotView::from))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRecall {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

/// Which orders a lot went out on. The reason lots exist: a batch is recalled
/// and somebody has to be told, by name, within the day.
pub async fn orders_for_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryLotId,
    query: ListRecall,
) -> Result<Page<LotShipmentView>> {
    let page = inventory::orders_for_lot(tx, ctx, id, paging(query.after.as_deref(), query.limit)?)
        .await?;

    Ok(map(page, LotShipmentView::from))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReserveFromLot {
    pub quantity: i32,
    pub line_item_id: Option<LineItemId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The operator naming which lot fills a hold, overriding what FEFO/FIFO
/// would have picked — for the customer who needs the later expiry.
pub async fn reserve_from_lot(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: InventoryLotId,
    body: ReserveFromLot,
) -> Result<ReservationView> {
    let made = inventory::reserve_from_lot(
        tx,
        ctx,
        id,
        body.quantity,
        body.line_item_id,
        body.expires_at,
    )
    .await?;

    Ok(ReservationView::from(made))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/inventory-items/{id}/tracking",
        action: Action::Write,
        domain: "inventory",
        query: None,
        summary: "Say how an item is counted and which lots leave first",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-items/{id}/lots",
        action: Action::Write,
        domain: "inventory",
        query: None,
        summary: "Receive a lot",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-items/{id}/lots",
        action: Action::View,
        domain: "inventory",
        query: Some(super::query_schema::<super::Paged>),
        summary: "List an item's lots",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-lots/expiring",
        action: Action::View,
        domain: "inventory",
        query: Some(super::query_schema::<super::Paged>),
        summary: "List what is about to go out of date",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-lots/{id}",
        action: Action::View,
        domain: "inventory",
        query: None,
        summary: "Fetch one lot",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/inventory-lots/{id}",
        action: Action::Write,
        domain: "inventory",
        query: None,
        summary: "Correct what is left in a lot",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/inventory-lots/{id}/orders",
        action: Action::View,
        domain: "inventory",
        query: None,
        summary: "Answer a recall: which orders this lot went out on",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/inventory-lots/{id}/reservations",
        action: Action::Write,
        domain: "inventory",
        query: None,
        summary: "Reserve a named lot, overriding FEFO/FIFO",
    },
];
