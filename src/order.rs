//! The order: what was bought, what it came to, and everything that happened
//! to it afterwards.
//!
//! Three rules hold the domain together.
//!
//! **An order is versioned.** `order_item` carries the quantities, one row per
//! `(order, version, line item)`, and an accepted change writes a fresh set at
//! `version + 1` without touching the old ones. What the order looked like
//! before an edit is still readable in full, which is what makes a dispute
//! answerable.
//!
//! **Money owed is a ledger.** `order_transaction` is signed — a capture is
//! positive, a refund negative — and the payment state is the sum rather than a
//! column somebody remembered to set. [`ledger`] is the only place it is added
//! up.
//!
//! **One mechanism changes an order.** [`request_change`], [`add_action`] and
//! [`confirm_change`] are it. Returns, exchanges and claims each open their own
//! rows and then hand the work to that mechanism; none of them is a second way
//! of moving a quantity.
//!
//! Totals come from [`crate::cart::compute`]. There is no second sum in this
//! module: an order that added itself up its own way would disagree with the
//! cart it came from, and the disagreement would arrive as a support ticket.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::cart::{CartTotals, TotalsLine, TotalsShipping, compute};
use crate::error::{Error, Result};
use crate::id::{
    AddressId, ClaimId, CustomerId, ExchangeId, LineItemId, OrderChangeId, OrderId, OrderItemId,
    OrderTransactionId, PaymentCollectionId, RegionId, ReturnId, SalesChannelId, ShippingOptionId,
    StockLocationId, VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const ORDER_COLUMNS: &str = "id, display_id, region_id, sales_channel_id, customer_id, \
                             shipping_address_id, billing_address_id, payment_collection_id, \
                             email, currency_code, locale, version, status, fulfillment_status, \
                             is_draft, no_notification, metadata, completed_at, canceled_at, \
                             created_at, updated_at";

const LINE_COLUMNS: &str = "id, order_id, variant_id, product_id, title, subtitle, thumbnail, \
                            product_title, product_handle, variant_title, variant_sku, \
                            variant_option_values, unit_price, compare_at_unit_price, \
                            currency_code, requires_shipping, is_tax_inclusive, is_discountable, \
                            is_giftcard, metadata, created_at, updated_at";

const ITEM_COLUMNS: &str = "id, order_id, order_line_item_id, version, unit_price, \
                            compare_at_unit_price, currency_code, quantity, fulfilled_quantity, \
                            shipped_quantity, delivered_quantity, return_requested_quantity, \
                            return_received_quantity, return_dismissed_quantity, \
                            written_off_quantity, metadata, created_at, updated_at";

const CHANGE_COLUMNS: &str = "id, order_id, order_return_id, order_exchange_id, order_claim_id, \
                              version, change_type, status, description, internal_note, \
                              created_by, requested_by, requested_at, confirmed_by, confirmed_at, \
                              declined_by, declined_at, declined_reason, metadata, created_at, \
                              updated_at";

const ACTION_COLUMNS: &str = "id, order_change_id, order_id, order_return_id, order_exchange_id, \
                              order_claim_id, version, ordering, action, reference, reference_id, \
                              details, amount, currency_code, internal_note, applied, created_at";

const RETURN_COLUMNS: &str = "id, order_id, order_version, display_id, status, location_id, \
                              refund_amount, currency_code, no_notification, created_by, \
                              requested_at, received_at, canceled_at, metadata, created_at, \
                              updated_at";

const EXCHANGE_COLUMNS: &str = "id, order_id, order_return_id, order_version, display_id, \
                                difference_due, currency_code, allow_backorder, no_notification, \
                                created_by, canceled_at, metadata, created_at, updated_at";

const CLAIM_COLUMNS: &str = "id, order_id, order_return_id, order_version, display_id, \
                             claim_type, refund_amount, currency_code, no_notification, \
                             created_by, canceled_at, metadata, created_at, updated_at";

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Where an order is in its life. Not where its money is: that is [`ledger`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Draft,
    Pending,
    /// The shopper has something left to do — a bank's second factor, most of
    /// the time — and the order is waiting for them rather than for the shop.
    RequiresAction,
    Completed,
    Canceled,
    Archived,
}

impl OrderStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            OrderStatus::Draft => "draft",
            OrderStatus::Pending => "pending",
            OrderStatus::RequiresAction => "requires_action",
            OrderStatus::Completed => "completed",
            OrderStatus::Canceled => "canceled",
            OrderStatus::Archived => "archived",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "draft" => OrderStatus::Draft,
            "pending" => OrderStatus::Pending,
            "requires_action" => OrderStatus::RequiresAction,
            "completed" => OrderStatus::Completed,
            "canceled" => OrderStatus::Canceled,
            "archived" => OrderStatus::Archived,
            _ => return Err(Error::bug("an order holds a status nothing writes")),
        })
    }

    pub fn is_final(self) -> bool {
        matches!(self, OrderStatus::Canceled | OrderStatus::Archived)
    }
}

/// Which moves are allowed.
///
/// A move to the status already held is allowed so that a retried request is a
/// no-op rather than a conflict; everything else is a one-way walk, and the two
/// final states have nothing after them.
pub fn can_transition(from: OrderStatus, to: OrderStatus) -> bool {
    use OrderStatus::*;

    if from == to {
        return true;
    }

    match from {
        Draft => matches!(to, Pending | Canceled),
        Pending => matches!(to, RequiresAction | Completed | Canceled),
        RequiresAction => matches!(to, Pending | Completed | Canceled),
        Completed => matches!(to, Archived | Canceled),
        Canceled | Archived => false,
    }
}

/// What the ledger says about money, worked out rather than stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentState {
    NotPaid,
    /// Something is held but not all of what is owed.
    PartiallyAuthorized,
    Authorized,
    PartiallyCaptured,
    Captured,
    PartiallyRefunded,
    Refunded,
}

impl PaymentState {
    pub const fn as_str(self) -> &'static str {
        match self {
            PaymentState::NotPaid => "not_paid",
            PaymentState::PartiallyAuthorized => "partially_authorized",
            PaymentState::Authorized => "authorized",
            PaymentState::PartiallyCaptured => "partially_captured",
            PaymentState::Captured => "captured",
            PaymentState::PartiallyRefunded => "partially_refunded",
            PaymentState::Refunded => "refunded",
        }
    }
}

/// The whole of the money against one order.
#[derive(Debug, Clone, Copy)]
pub struct Ledger {
    pub authorized: Money,
    pub captured: Money,
    pub refunded: Money,
    /// Captured less refunded: what the shop is actually holding.
    pub paid: Money,
    /// What the order comes to less what is paid. Negative means over-refunded.
    pub due: Money,
    pub state: PaymentState,
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub display_id: Option<i64>,
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    pub customer_id: Option<CustomerId>,
    pub shipping_address_id: Option<Uuid>,
    pub billing_address_id: Option<Uuid>,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub email: Option<String>,
    pub currency_code: String,
    pub locale: Option<String>,
    pub version: i32,
    pub status: String,
    pub fulfillment_status: String,
    pub is_draft: bool,
    pub no_notification: Option<bool>,
    pub metadata: Option<Value>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Order {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn status(&self) -> Result<OrderStatus> {
        OrderStatus::parse(&self.status)
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderLineItem {
    pub id: LineItemId,
    pub order_id: OrderId,
    pub variant_id: Option<VariantId>,
    pub product_id: Option<Uuid>,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail: Option<String>,
    pub product_title: Option<String>,
    pub product_handle: Option<String>,
    pub variant_title: Option<String>,
    pub variant_sku: Option<String>,
    pub variant_option_values: Option<Value>,
    pub unit_price: Decimal,
    pub compare_at_unit_price: Option<Decimal>,
    pub currency_code: String,
    pub requires_shipping: bool,
    pub is_tax_inclusive: bool,
    pub is_discountable: bool,
    pub is_giftcard: bool,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// One line at one version of the order, and the only place a quantity lives.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderItem {
    pub id: OrderItemId,
    pub order_id: OrderId,
    pub order_line_item_id: LineItemId,
    pub version: i32,
    pub unit_price: Option<Decimal>,
    pub compare_at_unit_price: Option<Decimal>,
    pub currency_code: String,
    pub quantity: i32,
    pub fulfilled_quantity: i32,
    pub shipped_quantity: i32,
    pub delivered_quantity: i32,
    pub return_requested_quantity: i32,
    pub return_received_quantity: i32,
    pub return_dismissed_quantity: i32,
    pub written_off_quantity: i32,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderShippingMethod {
    pub id: Uuid,
    pub order_id: OrderId,
    pub version: i32,
    pub name: String,
    pub description: Option<String>,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub amount: Decimal,
    pub currency_code: String,
    pub is_tax_inclusive: bool,
    pub data: Option<Value>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderSummary {
    pub id: Uuid,
    pub order_id: OrderId,
    pub version: i32,
    pub currency_code: String,
    pub totals: Value,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderTransaction {
    pub id: OrderTransactionId,
    pub order_id: OrderId,
    pub version: i32,
    pub amount: Decimal,
    pub currency_code: String,
    pub reference: Option<String>,
    pub reference_id: Option<Uuid>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderChange {
    pub id: OrderChangeId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_exchange_id: Option<ExchangeId>,
    pub order_claim_id: Option<ClaimId>,
    pub version: i32,
    pub change_type: String,
    pub status: String,
    pub description: Option<String>,
    pub internal_note: Option<String>,
    pub created_by: Option<String>,
    pub requested_by: Option<String>,
    pub requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub confirmed_by: Option<String>,
    pub confirmed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub declined_by: Option<String>,
    pub declined_at: Option<chrono::DateTime<chrono::Utc>>,
    pub declined_reason: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderChangeAction {
    pub id: Uuid,
    pub order_change_id: Option<OrderChangeId>,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_exchange_id: Option<ExchangeId>,
    pub order_claim_id: Option<ClaimId>,
    pub version: Option<i32>,
    pub ordering: i32,
    pub action: String,
    pub reference: Option<String>,
    pub reference_id: Option<Uuid>,
    pub details: Value,
    pub amount: Option<Decimal>,
    pub currency_code: Option<String>,
    pub internal_note: Option<String>,
    pub applied: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Return {
    pub id: ReturnId,
    pub order_id: OrderId,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub status: String,
    pub location_id: Option<Uuid>,
    pub refund_amount: Option<Decimal>,
    pub currency_code: String,
    pub no_notification: Option<bool>,
    pub created_by: Option<String>,
    pub requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub received_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ReturnItem {
    pub id: Uuid,
    pub order_return_id: ReturnId,
    pub order_line_item_id: LineItemId,
    pub return_reason_id: Option<Uuid>,
    pub quantity: i32,
    pub received_quantity: i32,
    pub damaged_quantity: i32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Exchange {
    pub id: ExchangeId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub difference_due: Option<Decimal>,
    pub currency_code: String,
    pub allow_backorder: bool,
    pub no_notification: Option<bool>,
    pub created_by: Option<String>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Claim {
    pub id: ClaimId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub claim_type: String,
    pub refund_amount: Option<Decimal>,
    pub currency_code: String,
    pub no_notification: Option<bool>,
    pub created_by: Option<String>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// What a caller hands in
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct OrderAddress {
    pub company: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub address_1: Option<String>,
    pub address_2: Option<String>,
    pub city: Option<String>,
    pub province: Option<String>,
    pub postal_code: Option<String>,
    pub country_code: Option<String>,
    pub phone: Option<String>,
}

/// Copies an address onto the order rather than pointing at one.
///
/// A customer editing their address book years later must not rewrite what a
/// parcel was sent to, so there is no foreign key here and nothing to follow.
async fn write_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: Option<CustomerId>,
    address: &OrderAddress,
) -> Result<AddressId> {
    let id = AddressId::new();

    sqlx::query(
        "insert into order_address
             (id, scope, customer_id, company, first_name, last_name, address_1, address_2,
              city, country_code, province, postal_code, phone)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(customer_id.map(|c| c.as_uuid()))
    .bind(address.company.as_deref())
    .bind(address.first_name.as_deref())
    .bind(address.last_name.as_deref())
    .bind(address.address_1.as_deref())
    .bind(address.address_2.as_deref())
    .bind(address.city.as_deref())
    .bind(address.country_code.as_deref().map(str::to_uppercase))
    .bind(address.province.as_deref())
    .bind(address.postal_code.as_deref())
    .bind(address.phone.as_deref())
    .execute(&mut **tx)
    .await?;

    Ok(id)
}

/// One line as the order will keep it: a snapshot, plus what the promotions
/// and the tax engine had already decided about it.
///
/// `discount` and `tax_rate` are carried rather than looked up because the
/// order has no adjustment tables of its own: they are written onto the item
/// and read back by [`totals`], so an order adds up to the same figure a year
/// later whatever has happened to the promotion since.
#[derive(Debug, Clone)]
pub struct NewOrderLine {
    pub variant_id: Option<VariantId>,
    pub product_id: Option<Uuid>,
    pub title: String,
    pub subtitle: Option<String>,
    pub thumbnail: Option<String>,
    pub product_title: Option<String>,
    pub product_handle: Option<String>,
    pub variant_title: Option<String>,
    pub variant_sku: Option<String>,
    pub variant_option_values: Option<Value>,
    pub quantity: i32,
    pub unit_price: Money,
    pub compare_at_unit_price: Option<Decimal>,
    pub is_tax_inclusive: bool,
    pub is_discountable: bool,
    pub requires_shipping: bool,
    pub discount: Decimal,
    /// The sum of the line's tax rates as a percentage: 18 is eighteen percent.
    pub tax_rate: Decimal,
}

impl NewOrderLine {
    pub fn of(title: impl Into<String>, quantity: i32, unit_price: Money) -> Self {
        NewOrderLine {
            variant_id: None,
            product_id: None,
            title: title.into(),
            subtitle: None,
            thumbnail: None,
            product_title: None,
            product_handle: None,
            variant_title: None,
            variant_sku: None,
            variant_option_values: None,
            quantity,
            unit_price,
            compare_at_unit_price: None,
            is_tax_inclusive: false,
            is_discountable: true,
            requires_shipping: true,
            discount: Decimal::ZERO,
            tax_rate: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewOrderShipping {
    pub name: String,
    pub description: Option<String>,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub amount: Money,
    pub is_tax_inclusive: bool,
    pub data: Option<Value>,
    pub discount: Decimal,
    pub tax_rate: Decimal,
}

#[derive(Debug, Clone)]
pub struct NewOrder {
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    pub customer_id: Option<CustomerId>,
    pub email: Option<String>,
    pub currency_code: Currency,
    pub locale: Option<String>,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub shipping_address: Option<OrderAddress>,
    pub billing_address: Option<OrderAddress>,
    pub lines: Vec<NewOrderLine>,
    pub shipping: Vec<NewOrderShipping>,
    pub no_notification: Option<bool>,
    pub metadata: Option<Value>,
}

impl NewOrder {
    pub fn of(currency_code: Currency) -> Self {
        NewOrder {
            region_id: None,
            sales_channel_id: None,
            customer_id: None,
            email: None,
            currency_code,
            locale: None,
            payment_collection_id: None,
            shipping_address: None,
            billing_address: None,
            lines: Vec::new(),
            shipping: Vec::new(),
            no_notification: None,
            metadata: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Creating and reading
// ---------------------------------------------------------------------------

/// Writes an order, its lines, its items at version 1 and its summary.
///
/// One call and therefore one transaction: an order with lines but no summary,
/// or with a summary and no items, is not a state anything else here knows how
/// to read.
pub async fn create(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewOrder) -> Result<Order> {
    place(tx, ctx, new, false).await
}

/// The same order, built in the back office and not yet anybody's to pay.
///
/// A draft is priced like every other order — same lines, same summary — so
/// sending it for payment is a status move rather than a second pricing pass.
pub async fn create_draft(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewOrder) -> Result<Order> {
    place(tx, ctx, new, true).await
}

async fn place(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewOrder, draft: bool) -> Result<Order> {
    let id = OrderId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: id.as_uuid(),
            customer: new.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if new.lines.is_empty() {
        return Err(Error::invalid("an order needs something on it"));
    }

    let currency = new.currency_code;
    for line in &new.lines {
        if line.quantity <= 0 {
            return Err(Error::invalid("a line needs a quantity of at least one"));
        }
        if line.unit_price.is_negative() {
            return Err(Error::invalid("a price cannot be negative"));
        }
        if line.unit_price.currency != currency {
            return Err(Error::invalid("that price is in another currency"));
        }
    }
    for method in &new.shipping {
        if method.amount.is_negative() {
            return Err(Error::invalid("a shipping price cannot be negative"));
        }
        if method.amount.currency != currency {
            return Err(Error::invalid("that price is in another currency"));
        }
    }

    let shipping_address_id = match new.shipping_address {
        Some(address) => Some(write_address(tx, ctx, new.customer_id, &address).await?),
        None => None,
    };
    let billing_address_id = match new.billing_address {
        Some(address) => Some(write_address(tx, ctx, new.customer_id, &address).await?),
        None => None,
    };

    let display_id: i64 = sqlx::query_scalar(
        r#"select coalesce(max(display_id), 0) + 1 from "order" where scope = $1"#,
    )
    .bind(ctx.scope.0)
    .fetch_one(&mut **tx)
    .await?;

    let status = if draft {
        OrderStatus::Draft
    } else {
        OrderStatus::Pending
    };

    let order = sqlx::query_as::<_, Order>(&format!(
        r#"insert into "order"
               (id, scope, display_id, region_id, sales_channel_id, customer_id,
                shipping_address_id, billing_address_id, payment_collection_id, email,
                currency_code, locale, status, is_draft, no_notification, metadata)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
           returning {ORDER_COLUMNS}"#
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(display_id)
    .bind(new.region_id.map(RegionId::as_uuid))
    .bind(new.sales_channel_id.map(SalesChannelId::as_uuid))
    .bind(new.customer_id.map(CustomerId::as_uuid))
    .bind(shipping_address_id)
    .bind(billing_address_id)
    .bind(new.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .bind(new.email.map(|value| value.trim().to_lowercase()))
    .bind(currency.as_str())
    .bind(new.locale)
    .bind(status.as_str())
    .bind(draft)
    .bind(new.no_notification)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    for line in new.lines {
        let line_id = LineItemId::new();
        sqlx::query(
            "insert into order_line_item
                 (id, scope, order_id, variant_id, product_id, title, subtitle, thumbnail,
                  product_title, product_handle, variant_title, variant_sku,
                  variant_option_values, unit_price, compare_at_unit_price, currency_code,
                  requires_shipping, is_tax_inclusive, is_discountable)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19)",
        )
        .bind(line_id.as_uuid())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(line.variant_id.map(VariantId::as_uuid))
        .bind(line.product_id)
        .bind(&line.title)
        .bind(&line.subtitle)
        .bind(&line.thumbnail)
        .bind(&line.product_title)
        .bind(&line.product_handle)
        .bind(&line.variant_title)
        .bind(&line.variant_sku)
        .bind(&line.variant_option_values)
        .bind(line.unit_price.amount)
        .bind(line.compare_at_unit_price)
        .bind(currency.as_str())
        .bind(line.requires_shipping)
        .bind(line.is_tax_inclusive)
        .bind(line.is_discountable)
        .execute(&mut **tx)
        .await?;

        insert_item(
            tx,
            ctx,
            id,
            line_id,
            1,
            line.quantity,
            line.unit_price.amount,
            currency,
            charges(line.discount, line.tax_rate),
        )
        .await?;
    }

    for method in new.shipping {
        insert_shipping(tx, ctx, id, 1, &method, currency).await?;
    }

    write_summary(tx, ctx, id, 1).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "display_id": display_id,
                "currency": currency.as_str(),
                "draft": draft,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: if draft {
                "order.draft_created"
            } else {
                "order.placed"
            },
            entity_id: id.as_uuid(),
            payload: serde_json::json!({ "display_id": display_id }),
        },
    )
    .await?;

    Ok(order)
}

pub async fn get(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<Order> {
    let order = read(tx, ctx, id).await?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    Ok(order)
}

/// Orders, newest last, optionally one customer's and optionally only drafts.
pub async fn list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: Option<CustomerId>,
    drafts: Option<bool>,
    paging: Paging,
) -> Result<Page<Order>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let rows = sqlx::query_as::<_, Order>(&format!(
        r#"select {ORDER_COLUMNS} from "order"
           where scope = $1
             and ($2::uuid is null or customer_id = $2)
             and ($3::boolean is null or is_draft = $3)
             and ($4::timestamptz is null or (created_at, id) > ($4, $5))
           order by created_at, id
           limit $6"#
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.map(CustomerId::as_uuid))
    .bind(drafts)
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

/// Most lines one order may be read with. An order with more than this is a
/// mistake somewhere upstream, not a page anybody wants.
pub const MAX_LINES: i64 = 500;

pub async fn line_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderLineItem>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderLineItem>(&format!(
        "select {LINE_COLUMNS} from order_line_item
         where scope = $1 and order_id = $2
         order by created_at, id limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

/// The items at one version. Asking for a version that has been superseded is
/// how the order is read as it was, and is not an error.
pub async fn items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<Vec<OrderItem>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderItem>(&format!(
        "select {ITEM_COLUMNS} from order_item
         where scope = $1 and order_id = $2 and version = $3
         order by created_at, id limit $4"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn shipping_methods(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<Vec<OrderShippingMethod>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderShippingMethod>(
        "select id, order_id, version, name, description, shipping_option_id, amount,
                currency_code, is_tax_inclusive, data, metadata
         from order_shipping_method
         where scope = $1 and order_id = $2 and version = $3
         order by created_at, id limit $4",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn summary(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<OrderSummary> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    sqlx::query_as::<_, OrderSummary>(
        "select id, order_id, version, currency_code, totals from order_summary
         where scope = $1 and order_id = $2 and version = $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order summary"))
}

// ---------------------------------------------------------------------------
// Totals
// ---------------------------------------------------------------------------

/// What one version of the order comes to.
///
/// The arithmetic is [`crate::cart::compute`] and nothing else, fed from the
/// item rows: the sum a shopper was shown and the sum an order is worth have
/// to come out of the same function or they will eventually differ.
pub async fn totals(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<CartTotals> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    add_up(tx, ctx, order_id, version).await
}

async fn add_up(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<CartTotals> {
    let order = read(tx, ctx, order_id).await?;
    let currency = order.currency()?;
    let exponent = exponent_of(tx, ctx, &order.currency_code).await?;

    let lines = sqlx::query_as::<_, TotalsLine>(
        "select i.quantity,
                coalesce(i.unit_price, l.unit_price) as unit_price,
                l.is_tax_inclusive,
                coalesce((i.metadata->>'discount')::numeric, 0) as discount,
                coalesce((i.metadata->>'tax_rate')::numeric, 0) as tax_rate
         from order_item i
         join order_line_item l on l.scope = i.scope and l.id = i.order_line_item_id
         where i.scope = $1 and i.order_id = $2 and i.version = $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .fetch_all(&mut **tx)
    .await?;

    let shipping = sqlx::query_as::<_, TotalsShipping>(
        "select amount, is_tax_inclusive,
                coalesce((metadata->>'discount')::numeric, 0) as discount,
                coalesce((metadata->>'tax_rate')::numeric, 0) as tax_rate
         from order_shipping_method
         where scope = $1 and order_id = $2 and version = $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .fetch_all(&mut **tx)
    .await?;

    compute(&lines, &shipping, currency, exponent)
}

/// Writes the summary for a version. Inserted beside the version, never
/// edited: a summary that disagrees with its rows has to stay visible.
pub async fn write_summary(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
) -> Result<OrderSummary> {
    let totals = add_up(tx, ctx, order_id, version).await?;

    let body = serde_json::json!({
        "subtotal": totals.subtotal.amount.to_string(),
        "discount": totals.discount.amount.to_string(),
        "shipping": totals.shipping.amount.to_string(),
        "tax": totals.tax.amount.to_string(),
        "total": totals.total.amount.to_string(),
    });

    sqlx::query_as::<_, OrderSummary>(
        "insert into order_summary (id, scope, order_id, version, currency_code, totals)
         values ($1, $2, $3, $4, $5, $6)
         on conflict (scope, order_id, version) do nothing
         returning id, order_id, version, currency_code, totals",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(totals.total.currency.as_str())
    .bind(&body)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that version already has a summary"))
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

pub async fn set_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    to: OrderStatus,
) -> Result<Order> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let from = order.status()?;
    if !can_transition(from, to) {
        return Err(Error::conflict(format!(
            "an order cannot go from {} to {}",
            from.as_str(),
            to.as_str()
        )));
    }
    if from == to {
        return Ok(order);
    }

    let now = ctx.now();
    let moved = sqlx::query_as::<_, Order>(&format!(
        r#"update "order" set
               status = $3,
               canceled_at = case when $3 = 'canceled' then coalesce(canceled_at, $4) end,
               completed_at = case when $3 = 'completed' then coalesce(completed_at, $4)
                                   else completed_at end,
               is_draft = case when $3 = 'draft' then is_draft else false end
           where scope = $1 and id = $2 and status = $5
           returning {ORDER_COLUMNS}"#
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(to.as_str())
    .bind(now)
    .bind(from.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that order moved while this was being decided"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order",
            entity_id: order_id.as_uuid(),
            summary: serde_json::json!({ "from": from.as_str(), "to": to.as_str() }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.status_changed",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "from": from.as_str(), "to": to.as_str() }),
        },
    )
    .await?;

    Ok(moved)
}

/// A draft leaves the back office: it stops being a draft and becomes an order
/// waiting to be paid. Nothing is re-priced — the summary written when it was
/// drawn up is the one it is sent out with.
pub async fn send_draft_for_payment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    payment_collection_id: PaymentCollectionId,
) -> Result<Order> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if !order.is_draft {
        return Err(Error::conflict("that order is not a draft"));
    }

    sqlx::query(r#"update "order" set payment_collection_id = $3 where scope = $1 and id = $2"#)
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(payment_collection_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    set_status(tx, ctx, order_id, OrderStatus::Pending).await
}

// ---------------------------------------------------------------------------
// The ledger
// ---------------------------------------------------------------------------

/// Writes one movement of money against the order.
///
/// `reference` and `reference_id` name what caused it, and the unique index on
/// them is why recording the same capture twice fails rather than paying twice.
pub async fn record_transaction(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    amount: Money,
    reference: &str,
    reference_id: Uuid,
) -> Result<OrderTransaction> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if amount.amount.is_zero() {
        return Err(Error::invalid("a transaction moves something"));
    }
    if amount.currency.as_str() != order.currency_code {
        return Err(Error::bug("a transaction met another currency"));
    }

    let written = sqlx::query_as::<_, OrderTransaction>(
        "insert into order_transaction
             (id, scope, order_id, version, amount, currency_code, reference, reference_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         on conflict (scope, order_id, reference, reference_id) where reference is not null do nothing
         returning id, order_id, version, amount, currency_code, reference, reference_id,
                   metadata, created_at",
    )
    .bind(OrderTransactionId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order.version)
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(reference)
    .bind(reference_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that movement is already in the ledger"))?;

    ctx.emit(
        tx,
        Event {
            name: "order.transaction_recorded",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({
                "amount": amount.amount.to_string(),
                "reference": reference,
            }),
        },
    )
    .await?;

    Ok(written)
}

pub async fn transactions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderTransaction>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderTransaction>(
        "select id, order_id, version, amount, currency_code, reference, reference_id,
                metadata, created_at
         from order_transaction
         where scope = $1 and order_id = $2
         order by created_at, id limit $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

/// The payment state, added up from the ledger. There is no column holding it:
/// two writers and a webhook would each have their own idea of what it should
/// say, and the sum has only one.
pub async fn ledger(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<Ledger> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let order = read(tx, ctx, order_id).await?;
    let currency = order.currency()?;
    let totals = add_up(tx, ctx, order_id, order.version).await?;

    let (authorized, captured, refunded): (Decimal, Decimal, Decimal) = sqlx::query_as(
        "select
             coalesce(sum(amount) filter (where reference = 'payment'), 0),
             coalesce(sum(amount) filter (where reference in ('capture', 'manual')), 0),
             coalesce(-sum(amount) filter (where reference in ('refund', 'order_return',
                                                               'order_exchange', 'order_claim')), 0)
         from order_transaction
         where scope = $1 and order_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    let owed = totals.total.amount;
    let paid = captured - refunded;

    let state = if refunded > Decimal::ZERO && refunded >= captured {
        PaymentState::Refunded
    } else if refunded > Decimal::ZERO {
        PaymentState::PartiallyRefunded
    } else if captured >= owed && captured > Decimal::ZERO {
        PaymentState::Captured
    } else if captured > Decimal::ZERO {
        PaymentState::PartiallyCaptured
    } else if authorized >= owed && authorized > Decimal::ZERO {
        PaymentState::Authorized
    } else if authorized > Decimal::ZERO {
        PaymentState::PartiallyAuthorized
    } else {
        PaymentState::NotPaid
    };

    Ok(Ledger {
        authorized: Money::new(authorized, currency),
        captured: Money::new(captured, currency),
        refunded: Money::new(refunded, currency),
        paid: Money::new(paid, currency),
        due: Money::new(owed - paid, currency),
        state,
    })
}

// ---------------------------------------------------------------------------
// Changes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeType {
    Edit,
    Return,
    Exchange,
    Claim,
}

impl ChangeType {
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeType::Edit => "edit",
            ChangeType::Return => "return",
            ChangeType::Exchange => "exchange",
            ChangeType::Claim => "claim",
        }
    }
}

/// One act a change performs. Only the ones this module knows how to carry
/// into the next version are here; fulfilment writes its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeAction {
    ItemAdd,
    ItemUpdate,
    ItemRemove,
    ReturnItem,
    ReceiveReturnItem,
    ReceiveDamagedReturnItem,
    DismissReturnItem,
    WriteOffItem,
    ShippingAdd,
    ShippingRemove,
    CreditLineAdd,
}

impl ChangeAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeAction::ItemAdd => "ITEM_ADD",
            ChangeAction::ItemUpdate => "ITEM_UPDATE",
            ChangeAction::ItemRemove => "ITEM_REMOVE",
            ChangeAction::ReturnItem => "RETURN_ITEM",
            ChangeAction::ReceiveReturnItem => "RECEIVE_RETURN_ITEM",
            ChangeAction::ReceiveDamagedReturnItem => "RECEIVE_DAMAGED_RETURN_ITEM",
            ChangeAction::DismissReturnItem => "DISMISS_RETURN_ITEM",
            ChangeAction::WriteOffItem => "WRITE_OFF_ITEM",
            ChangeAction::ShippingAdd => "SHIPPING_ADD",
            ChangeAction::ShippingRemove => "SHIPPING_REMOVE",
            ChangeAction::CreditLineAdd => "CREDIT_LINE_ADD",
        }
    }

    fn parse(text: &str) -> Result<Self> {
        Ok(match text {
            "ITEM_ADD" => ChangeAction::ItemAdd,
            "ITEM_UPDATE" => ChangeAction::ItemUpdate,
            "ITEM_REMOVE" => ChangeAction::ItemRemove,
            "RETURN_ITEM" => ChangeAction::ReturnItem,
            "RECEIVE_RETURN_ITEM" => ChangeAction::ReceiveReturnItem,
            "RECEIVE_DAMAGED_RETURN_ITEM" => ChangeAction::ReceiveDamagedReturnItem,
            "DISMISS_RETURN_ITEM" => ChangeAction::DismissReturnItem,
            "WRITE_OFF_ITEM" => ChangeAction::WriteOffItem,
            "SHIPPING_ADD" => ChangeAction::ShippingAdd,
            "SHIPPING_REMOVE" => ChangeAction::ShippingRemove,
            "CREDIT_LINE_ADD" => ChangeAction::CreditLineAdd,
            _ => {
                return Err(Error::invalid(
                    "that action is not one this module can confirm",
                ));
            }
        })
    }
}

/// What to do, and to which line.
///
/// `details` carries the rest: `quantity` for anything counting, `unit_price`
/// and `currency_code` for an addition, `name` and `amount` for shipping.
#[derive(Debug, Clone)]
pub struct NewAction {
    pub action: ChangeAction,
    pub order_line_item_id: Option<LineItemId>,
    pub details: Value,
    pub amount: Option<Money>,
    pub internal_note: Option<String>,
}

impl NewAction {
    pub fn on(action: ChangeAction, line: LineItemId, quantity: i32) -> Self {
        NewAction {
            action,
            order_line_item_id: Some(line),
            details: serde_json::json!({ "quantity": quantity }),
            amount: None,
            internal_note: None,
        }
    }
}

/// Opens a change. The partial unique index on `(scope, order_id)` for the
/// unsettled statuses is what stops two people editing one order at once.
pub async fn request_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    change_type: ChangeType,
    description: Option<String>,
) -> Result<OrderChange> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if order.status()?.is_final() {
        return Err(Error::conflict("that order is closed"));
    }

    let id = OrderChangeId::new();
    let change = sqlx::query_as::<_, OrderChange>(&format!(
        "insert into order_change
             (id, scope, order_id, version, change_type, status, description, requested_at)
         values ($1, $2, $3, $4, $5, 'requested', $6, $7)
         returning {CHANGE_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order.version + 1)
    .bind(change_type.as_str())
    .bind(description)
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_change",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "order": order_id, "type": change_type.as_str() }),
        },
    )
    .await?;

    Ok(change)
}

pub async fn add_action(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change_id: OrderChangeId,
    new: NewAction,
) -> Result<OrderChangeAction> {
    let change = read_change(tx, ctx, change_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: change.order_id.as_uuid(),
            customer: None,
        },
    )?;

    if change.status != "pending" && change.status != "requested" {
        return Err(Error::conflict("that change has already been settled"));
    }

    let ordering: i32 = sqlx::query_scalar(
        "select coalesce(max(ordering), -1) + 1 from order_change_action
         where scope = $1 and order_change_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    let (reference, reference_id) = match new.order_line_item_id {
        Some(line) => (Some("order_line_item"), Some(line.as_uuid())),
        None => (None, None),
    };

    Ok(sqlx::query_as::<_, OrderChangeAction>(&format!(
        "insert into order_change_action
             (id, scope, order_change_id, order_id, order_return_id, order_exchange_id,
              order_claim_id, ordering, action, reference, reference_id, details, amount,
              currency_code, internal_note)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         returning {ACTION_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .bind(change.order_id.as_uuid())
    .bind(change.order_return_id.map(ReturnId::as_uuid))
    .bind(change.order_exchange_id.map(ExchangeId::as_uuid))
    .bind(change.order_claim_id.map(ClaimId::as_uuid))
    .bind(ordering)
    .bind(new.action.as_str())
    .bind(reference)
    .bind(reference_id)
    .bind(&new.details)
    .bind(new.amount.map(|money| money.amount))
    .bind(new.amount.map(|money| money.currency.as_str().to_string()))
    .bind(new.internal_note)
    .fetch_one(&mut **tx)
    .await?)
}

/// Applies a change, in order, at a new version.
///
/// The previous version's item rows are copied forward and the actions land on
/// the copies. Nothing at the old version is touched, so the order as it stood
/// before the edit stays readable — which is the whole reason the table is
/// keyed by version.
pub async fn confirm_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change_id: OrderChangeId,
) -> Result<Order> {
    let change = read_change(tx, ctx, change_id).await?;
    let order = read(tx, ctx, change.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if change.status != "pending" && change.status != "requested" {
        return Err(Error::conflict("that change has already been settled"));
    }

    let next = order.version + 1;
    let currency = order.currency()?;

    carry_forward(tx, ctx, order.id, order.version, next).await?;

    let actions = sqlx::query_as::<_, OrderChangeAction>(&format!(
        "select {ACTION_COLUMNS} from order_change_action
         where scope = $1 and order_change_id = $2 and not applied
         order by ordering"
    ))
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for action in &actions {
        apply_action(tx, ctx, &order, next, currency, action).await?;

        sqlx::query(
            "update order_change_action set applied = true, version = $3
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(action.id)
        .bind(next)
        .execute(&mut **tx)
        .await?;
    }

    let moved = sqlx::query_as::<_, Order>(&format!(
        r#"update "order" set version = $3
           where scope = $1 and id = $2 and version = $4
           returning {ORDER_COLUMNS}"#
    ))
    .bind(ctx.scope.0)
    .bind(order.id.as_uuid())
    .bind(next)
    .bind(order.version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that order changed while this was being applied"))?;

    write_summary(tx, ctx, order.id, next).await?;

    sqlx::query(
        "update order_change set status = 'confirmed', confirmed_at = $3, confirmed_by = $4
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .bind(ctx.now())
    .bind(actor_name(ctx))
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_change",
            entity_id: change_id.as_uuid(),
            summary: serde_json::json!({ "order": order.id, "version": next }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.changed",
            entity_id: order.id.as_uuid(),
            payload: serde_json::json!({
                "change": change_id,
                "type": change.change_type,
                "version": next,
            }),
        },
    )
    .await?;

    Ok(moved)
}

pub async fn decline_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change_id: OrderChangeId,
    reason: Option<String>,
) -> Result<OrderChange> {
    let change = read_change(tx, ctx, change_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: change.order_id.as_uuid(),
            customer: None,
        },
    )?;

    if change.status != "pending" && change.status != "requested" {
        return Err(Error::conflict("that change has already been settled"));
    }

    let declined = sqlx::query_as::<_, OrderChange>(&format!(
        "update order_change
         set status = 'declined', declined_at = $3, declined_by = $4, declined_reason = $5
         where scope = $1 and id = $2
         returning {CHANGE_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .bind(ctx.now())
    .bind(actor_name(ctx))
    .bind(reason)
    .fetch_one(&mut **tx)
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.change_declined",
            entity_id: change.order_id.as_uuid(),
            payload: serde_json::json!({ "change": change_id }),
        },
    )
    .await?;

    Ok(declined)
}

pub async fn changes(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    paging: Paging,
) -> Result<Page<OrderChange>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change
         where scope = $1
           and order_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
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

pub async fn change_actions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change_id: OrderChangeId,
) -> Result<Vec<OrderChangeAction>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderChangeAction>(&format!(
        "select {ACTION_COLUMNS} from order_change_action
         where scope = $1 and order_change_id = $2
         order by ordering limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ReturnLine {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub return_reason_id: Option<Uuid>,
    pub note: Option<String>,
}

/// Somebody wants to send something back.
///
/// The return rows record the intent and an [`OrderChange`] of type `return`
/// carries it into the order's next version, where `return_requested_quantity`
/// goes up. There is no second path that moves that number.
pub async fn request_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    location_id: Option<StockLocationId>,
    lines: Vec<ReturnLine>,
) -> Result<Return> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if lines.is_empty() {
        return Err(Error::invalid("a return needs something on it"));
    }

    let order_return = open_return(tx, ctx, &order, location_id, &lines).await?;

    let change = request_change(tx, ctx, order_id, ChangeType::Return, None).await?;
    attach_change(tx, ctx, change.id, Some(order_return.id), None, None).await?;

    for line in &lines {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::ReturnItem,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;
    }

    confirm_change(tx, ctx, change.id).await?;

    ctx.emit(
        tx,
        Event {
            name: "order.return_requested",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "return": order_return.id }),
        },
    )
    .await?;

    Ok(order_return)
}

#[derive(Debug, Clone)]
pub struct ReceivedLine {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    /// Of the received quantity, how much came back unsellable. Damaged stock
    /// is counted as received but never put back on the shelf.
    pub damaged: i32,
}

/// The parcel arrived. The quantities move, and what is sellable goes back
/// into stock at the return's location.
pub async fn receive_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
    lines: Vec<ReceivedLine>,
) -> Result<Return> {
    let order_return = read_return(tx, ctx, return_id).await?;
    let order = read(tx, ctx, order_return.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if order_return.status == "received" || order_return.canceled_at.is_some() {
        return Err(Error::conflict("that return is already settled"));
    }
    if lines.is_empty() {
        return Err(Error::invalid("nothing was received"));
    }

    let change = request_change(tx, ctx, order.id, ChangeType::Return, None).await?;
    attach_change(tx, ctx, change.id, Some(return_id), None, None).await?;

    for line in &lines {
        if line.quantity <= 0 || line.damaged < 0 || line.damaged > line.quantity {
            return Err(Error::invalid("that is not a quantity received"));
        }

        let moved = sqlx::query(
            "update order_return_item
             set received_quantity = received_quantity + $3,
                 damaged_quantity = damaged_quantity + $4
             where scope = $1 and id = (
                 select id from order_return_item
                 where scope = $1 and order_return_id = $2 and order_line_item_id = $5
             )",
        )
        .bind(ctx.scope.0)
        .bind(return_id.as_uuid())
        .bind(line.quantity)
        .bind(line.damaged)
        .bind(line.order_line_item_id.as_uuid())
        .execute(&mut **tx)
        .await?;

        if moved.rows_affected() == 0 {
            return Err(Error::conflict("that is more than the return asked for"));
        }

        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::ReceiveReturnItem,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;

        let sellable = line.quantity - line.damaged;
        if sellable > 0 {
            restock(tx, ctx, &order_return, line.order_line_item_id, sellable).await?;
        }
    }

    confirm_change(tx, ctx, change.id).await?;

    let outstanding: i64 = sqlx::query_scalar(
        "select count(*) from order_return_item
         where scope = $1 and order_return_id = $2 and received_quantity < quantity",
    )
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    let status = if outstanding == 0 {
        "received"
    } else {
        "partially_received"
    };

    let settled = sqlx::query_as::<_, Return>(&format!(
        "update order_return
         set status = $3, received_at = case when $3 = 'received' then $4 else received_at end
         where scope = $1 and id = $2
         returning {RETURN_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .bind(status)
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.return_received",
            entity_id: order.id.as_uuid(),
            payload: serde_json::json!({ "return": return_id, "status": status }),
        },
    )
    .await?;

    Ok(settled)
}

/// Something came back that the shop is not taking: it is counted as received
/// and then dismissed, and no stock moves.
pub async fn dismiss_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
    lines: Vec<ReceivedLine>,
) -> Result<Return> {
    let order_return = read_return(tx, ctx, return_id).await?;
    let order = read(tx, ctx, order_return.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if lines.is_empty() {
        return Err(Error::invalid("nothing was dismissed"));
    }

    let change = request_change(tx, ctx, order.id, ChangeType::Return, None).await?;
    attach_change(tx, ctx, change.id, Some(return_id), None, None).await?;

    for line in &lines {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::DismissReturnItem,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;
    }

    confirm_change(tx, ctx, change.id).await?;

    ctx.emit(
        tx,
        Event {
            name: "order.return_dismissed",
            entity_id: order.id.as_uuid(),
            payload: serde_json::json!({ "return": return_id }),
        },
    )
    .await?;

    read_return(tx, ctx, return_id).await
}

/// Why somebody is sending something back. Shop configuration rather than
/// anybody's data, which is why a shopper may read it.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ReturnReason {
    pub id: Uuid,
    pub parent_return_reason_id: Option<Uuid>,
    pub value: String,
    pub label: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const RETURN_REASON_COLUMNS: &str =
    "id, parent_return_reason_id, value, label, description, created_at";

pub async fn return_reasons(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<ReturnReason>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, ReturnReason>(&format!(
        "select {RETURN_REASON_COLUMNS} from return_reason
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
        id: row.id,
    }))
}

pub async fn return_reason(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: Uuid) -> Result<ReturnReason> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: None,
        },
    )?;

    sqlx::query_as::<_, ReturnReason>(&format!(
        "select {RETURN_REASON_COLUMNS} from return_reason where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("return reason"))
}

pub async fn returns(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    paging: Paging,
) -> Result<Page<Return>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, Return>(&format!(
        "select {RETURN_COLUMNS} from order_return
         where scope = $1
           and order_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
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

pub async fn return_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
) -> Result<Vec<ReturnItem>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: Uuid::nil(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, ReturnItem>(
        "select id, order_return_id, order_line_item_id, return_reason_id, quantity,
                received_quantity, damaged_quantity, note
         from order_return_item
         where scope = $1 and order_return_id = $2
         order by created_at, id limit $3",
    )
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

// ---------------------------------------------------------------------------
// Exchanges
// ---------------------------------------------------------------------------

/// What goes back and what goes out instead. The outbound lines are already
/// on the order as `order_line_item` rows — an exchange adds quantity at the
/// next version rather than inventing a second kind of line.
#[derive(Debug, Clone)]
pub struct ExchangeRequest {
    pub returning: Vec<ReturnLine>,
    pub outbound: Vec<ExchangeLine>,
    pub location_id: Option<StockLocationId>,
    pub allow_backorder: bool,
    /// What is still owed either way once both halves are priced. Positive is
    /// owed by the customer.
    pub difference_due: Option<Money>,
}

#[derive(Debug, Clone)]
pub struct ExchangeLine {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub note: Option<String>,
}

/// The return and the outgoing items settle as one change, so a rollback takes
/// both: an exchange that recorded the return and lost the replacement is a
/// customer owed goods nobody knows about.
pub async fn request_exchange(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    request: ExchangeRequest,
) -> Result<Exchange> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if request.returning.is_empty() || request.outbound.is_empty() {
        return Err(Error::invalid("an exchange has both halves"));
    }

    let order_return =
        open_return(tx, ctx, &order, request.location_id, &request.returning).await?;

    let id = ExchangeId::new();
    let display_id = next_display(tx, ctx, "order_exchange").await?;
    let exchange = sqlx::query_as::<_, Exchange>(&format!(
        "insert into order_exchange
             (id, scope, order_id, order_return_id, order_version, display_id, difference_due,
              currency_code, allow_backorder, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning {EXCHANGE_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order_return.id.as_uuid())
    .bind(order.version)
    .bind(display_id)
    .bind(request.difference_due.map(|money| money.amount))
    .bind(&order.currency_code)
    .bind(request.allow_backorder)
    .bind(actor_name(ctx))
    .fetch_one(&mut **tx)
    .await?;

    for line in &request.outbound {
        sqlx::query(
            "insert into order_exchange_item
                 (id, scope, order_exchange_id, order_line_item_id, quantity, note)
             values ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(line.order_line_item_id.as_uuid())
        .bind(line.quantity)
        .bind(&line.note)
        .execute(&mut **tx)
        .await?;
    }

    let change = request_change(tx, ctx, order_id, ChangeType::Exchange, None).await?;
    attach_change(tx, ctx, change.id, Some(order_return.id), Some(id), None).await?;

    for line in &request.returning {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::ReturnItem,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;
    }
    for line in &request.outbound {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::ItemAdd,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;
    }

    confirm_change(tx, ctx, change.id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_exchange",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "order": order_id, "return": order_return.id }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.exchange_requested",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "exchange": id }),
        },
    )
    .await?;

    Ok(exchange)
}

// ---------------------------------------------------------------------------
// Claims
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaimType {
    /// Money back, and the goods written off rather than expected back.
    Refund,
    /// Something else sent instead.
    Replace,
}

impl ClaimType {
    pub const fn as_str(self) -> &'static str {
        match self {
            ClaimType::Refund => "refund",
            ClaimType::Replace => "replace",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClaimLine {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    /// `missing_item`, `wrong_item`, `production_failure` or `other`.
    pub reason: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaimRequest {
    pub claim_type: ClaimType,
    /// What went wrong.
    pub faulty: Vec<ClaimLine>,
    /// What is being sent instead, for a `replace`.
    pub replacements: Vec<ClaimLine>,
    /// Whether the faulty goods are wanted back. A damaged parcel usually is
    /// not, and then the quantity is written off where it stands.
    pub collect: bool,
    pub location_id: Option<StockLocationId>,
    pub refund_amount: Option<Money>,
}

/// Damaged or missing: replaced, or refunded.
///
/// This is [`request_change`] with claim rows beside it and nothing else. A
/// claim that moved a quantity its own way would be a second mechanism, and
/// the two would disagree the first time one of them was fixed.
pub async fn request_claim(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    request: ClaimRequest,
) -> Result<Claim> {
    let order = read(tx, ctx, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if request.faulty.is_empty() {
        return Err(Error::invalid("a claim needs something wrong with it"));
    }
    if request.claim_type == ClaimType::Replace && request.replacements.is_empty() {
        return Err(Error::invalid("a replacement claim needs a replacement"));
    }

    let order_return = if request.collect {
        let lines: Vec<ReturnLine> = request
            .faulty
            .iter()
            .map(|line| ReturnLine {
                order_line_item_id: line.order_line_item_id,
                quantity: line.quantity,
                return_reason_id: None,
                note: line.note.clone(),
            })
            .collect();
        Some(open_return(tx, ctx, &order, request.location_id, &lines).await?)
    } else {
        None
    };

    let id = ClaimId::new();
    let display_id = next_display(tx, ctx, "order_claim").await?;
    let claim = sqlx::query_as::<_, Claim>(&format!(
        "insert into order_claim
             (id, scope, order_id, order_return_id, order_version, display_id, claim_type,
              refund_amount, currency_code, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning {CLAIM_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order_return.as_ref().map(|row| row.id.as_uuid()))
    .bind(order.version)
    .bind(display_id)
    .bind(request.claim_type.as_str())
    .bind(request.refund_amount.map(|money| money.amount))
    .bind(&order.currency_code)
    .bind(actor_name(ctx))
    .fetch_one(&mut **tx)
    .await?;

    for (line, additional) in request
        .faulty
        .iter()
        .map(|line| (line, false))
        .chain(request.replacements.iter().map(|line| (line, true)))
    {
        sqlx::query(
            "insert into order_claim_item
                 (id, scope, order_claim_id, order_line_item_id, reason, quantity,
                  is_additional_item, note)
             values ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(line.order_line_item_id.as_uuid())
        .bind(&line.reason)
        .bind(line.quantity)
        .bind(additional)
        .bind(&line.note)
        .execute(&mut **tx)
        .await?;
    }

    let change = request_change(tx, ctx, order_id, ChangeType::Claim, None).await?;
    attach_change(
        tx,
        ctx,
        change.id,
        order_return.as_ref().map(|row| row.id),
        None,
        Some(id),
    )
    .await?;

    for line in &request.faulty {
        let action = if request.collect {
            ChangeAction::ReturnItem
        } else {
            ChangeAction::WriteOffItem
        };
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(action, line.order_line_item_id, line.quantity),
        )
        .await?;
    }
    for line in &request.replacements {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction::on(
                ChangeAction::ItemAdd,
                line.order_line_item_id,
                line.quantity,
            ),
        )
        .await?;
    }
    if let Some(amount) = request.refund_amount {
        add_action(
            tx,
            ctx,
            change.id,
            NewAction {
                action: ChangeAction::CreditLineAdd,
                order_line_item_id: None,
                details: serde_json::json!({}),
                amount: Some(amount),
                internal_note: None,
            },
        )
        .await?;
    }

    confirm_change(tx, ctx, change.id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_claim",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "order": order_id, "type": request.claim_type.as_str() }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.claim_requested",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "claim": id }),
        },
    )
    .await?;

    Ok(claim)
}

// ---------------------------------------------------------------------------
// Applying one action
// ---------------------------------------------------------------------------

async fn apply_action(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: &Order,
    version: i32,
    currency: Currency,
    action: &OrderChangeAction,
) -> Result<()> {
    let what = ChangeAction::parse(&action.action)?;
    let line = action
        .reference_id
        .map(LineItemId::from_uuid)
        .ok_or_else(|| Error::invalid("that action names no line"));

    match what {
        ChangeAction::ItemAdd => {
            let line = line?;
            let quantity = quantity_of(&action.details)?;
            let price = match action.amount {
                Some(amount) => amount,
                None => line_price(tx, ctx, line).await?,
            };

            let added = sqlx::query(
                "update order_item set quantity = quantity + $4
                 where scope = $1 and order_id = $2 and version = $3
                   and order_line_item_id = $5",
            )
            .bind(ctx.scope.0)
            .bind(order.id.as_uuid())
            .bind(version)
            .bind(quantity)
            .bind(line.as_uuid())
            .execute(&mut **tx)
            .await?;

            if added.rows_affected() == 0 {
                insert_item(
                    tx,
                    ctx,
                    order.id,
                    line,
                    version,
                    quantity,
                    price,
                    currency,
                    charges(Decimal::ZERO, Decimal::ZERO),
                )
                .await?;
            }
        }
        ChangeAction::ItemUpdate => {
            let line = line?;
            let quantity = quantity_of(&action.details)?;
            touch(
                tx,
                ctx,
                order.id,
                version,
                line,
                "quantity = $4",
                quantity,
                "that line is not on this order",
            )
            .await?;
        }
        ChangeAction::ItemRemove => {
            let line = line?;
            sqlx::query(
                "delete from order_item
                 where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $4",
            )
            .bind(ctx.scope.0)
            .bind(order.id.as_uuid())
            .bind(version)
            .bind(line.as_uuid())
            .execute(&mut **tx)
            .await?;
        }
        ChangeAction::ReturnItem => {
            bump(
                tx,
                ctx,
                order.id,
                version,
                line?,
                "return_requested_quantity",
                quantity_of(&action.details)?,
            )
            .await?;
        }
        ChangeAction::ReceiveReturnItem | ChangeAction::ReceiveDamagedReturnItem => {
            bump(
                tx,
                ctx,
                order.id,
                version,
                line?,
                "return_received_quantity",
                quantity_of(&action.details)?,
            )
            .await?;
        }
        ChangeAction::DismissReturnItem => {
            bump(
                tx,
                ctx,
                order.id,
                version,
                line?,
                "return_dismissed_quantity",
                quantity_of(&action.details)?,
            )
            .await?;
        }
        ChangeAction::WriteOffItem => {
            bump(
                tx,
                ctx,
                order.id,
                version,
                line?,
                "written_off_quantity",
                quantity_of(&action.details)?,
            )
            .await?;
        }
        ChangeAction::ShippingAdd => {
            let amount = action
                .amount
                .ok_or_else(|| Error::invalid("added shipping needs a price"))?;
            let name = action
                .details
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Shipping")
                .to_string();

            insert_shipping(
                tx,
                ctx,
                order.id,
                version,
                &NewOrderShipping {
                    name,
                    description: None,
                    shipping_option_id: None,
                    amount: Money::new(amount, currency),
                    is_tax_inclusive: false,
                    data: None,
                    discount: Decimal::ZERO,
                    tax_rate: Decimal::ZERO,
                },
                currency,
            )
            .await?;
        }
        ChangeAction::ShippingRemove => {
            let name = action
                .details
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::invalid("removed shipping needs a name"))?;

            sqlx::query(
                "delete from order_shipping_method
                 where scope = $1 and order_id = $2 and version = $3 and name = $4",
            )
            .bind(ctx.scope.0)
            .bind(order.id.as_uuid())
            .bind(version)
            .bind(name)
            .execute(&mut **tx)
            .await?;
        }
        ChangeAction::CreditLineAdd => {
            let amount = action
                .amount
                .ok_or_else(|| Error::invalid("a credit line needs an amount"))?;

            sqlx::query(
                "insert into order_credit_line
                     (id, scope, order_id, version, amount, currency_code)
                 values ($1, $2, $3, $4, $5, $6)",
            )
            .bind(Uuid::now_v7())
            .bind(ctx.scope.0)
            .bind(order.id.as_uuid())
            .bind(version)
            .bind(amount)
            .bind(currency.as_str())
            .execute(&mut **tx)
            .await?;
        }
    }

    Ok(())
}

/// Copies a version's items and shipping forward. The old rows stay exactly as
/// they were.
async fn carry_forward(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    from: i32,
    to: i32,
) -> Result<()> {
    let items = sqlx::query_as::<_, OrderItem>(&format!(
        "select {ITEM_COLUMNS} from order_item
         where scope = $1 and order_id = $2 and version = $3
         order by created_at, id"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(from)
    .fetch_all(&mut **tx)
    .await?;

    for item in items {
        sqlx::query(
            "insert into order_item
                 (id, scope, order_id, order_line_item_id, version, unit_price,
                  compare_at_unit_price, currency_code, quantity, fulfilled_quantity,
                  shipped_quantity, delivered_quantity, return_requested_quantity,
                  return_received_quantity, return_dismissed_quantity, written_off_quantity,
                  metadata)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(OrderItemId::new().as_uuid())
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(item.order_line_item_id.as_uuid())
        .bind(to)
        .bind(item.unit_price)
        .bind(item.compare_at_unit_price)
        .bind(&item.currency_code)
        .bind(item.quantity)
        .bind(item.fulfilled_quantity)
        .bind(item.shipped_quantity)
        .bind(item.delivered_quantity)
        .bind(item.return_requested_quantity)
        .bind(item.return_received_quantity)
        .bind(item.return_dismissed_quantity)
        .bind(item.written_off_quantity)
        .bind(&item.metadata)
        .execute(&mut **tx)
        .await?;
    }

    let methods = sqlx::query_as::<_, OrderShippingMethod>(
        "select id, order_id, version, name, description, shipping_option_id, amount,
                currency_code, is_tax_inclusive, data, metadata
         from order_shipping_method
         where scope = $1 and order_id = $2 and version = $3
         order by created_at, id",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(from)
    .fetch_all(&mut **tx)
    .await?;

    for method in methods {
        sqlx::query(
            "insert into order_shipping_method
                 (id, scope, order_id, version, name, description, shipping_option_id, amount,
                  currency_code, is_tax_inclusive, data, metadata)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(to)
        .bind(&method.name)
        .bind(&method.description)
        .bind(method.shipping_option_id.map(ShippingOptionId::as_uuid))
        .bind(method.amount)
        .bind(&method.currency_code)
        .bind(method.is_tax_inclusive)
        .bind(&method.data)
        .bind(&method.metadata)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// The small shared parts
// ---------------------------------------------------------------------------

async fn read(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<Order> {
    sqlx::query_as::<_, Order>(&format!(
        r#"select {ORDER_COLUMNS} from "order" where scope = $1 and id = $2"#
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order"))
}

async fn read_change(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderChangeId) -> Result<OrderChange> {
    sqlx::query_as::<_, OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order change"))
}

async fn read_return(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReturnId) -> Result<Return> {
    sqlx::query_as::<_, Return>(&format!(
        "select {RETURN_COLUMNS} from order_return where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("return"))
}

async fn open_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order: &Order,
    location_id: Option<StockLocationId>,
    lines: &[ReturnLine],
) -> Result<Return> {
    let id = ReturnId::new();
    let display_id = next_display(tx, ctx, "order_return").await?;

    let order_return = sqlx::query_as::<_, Return>(&format!(
        "insert into order_return
             (id, scope, order_id, order_version, display_id, status, location_id,
              currency_code, created_by, requested_at)
         values ($1, $2, $3, $4, $5, 'requested', $6, $7, $8, $9)
         returning {RETURN_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order.id.as_uuid())
    .bind(order.version)
    .bind(display_id)
    .bind(location_id.map(StockLocationId::as_uuid))
    .bind(&order.currency_code)
    .bind(actor_name(ctx))
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;

    for line in lines {
        if line.quantity <= 0 {
            return Err(Error::invalid("a return line needs a quantity"));
        }

        sqlx::query(
            "insert into order_return_item
                 (id, scope, order_return_id, order_line_item_id, return_reason_id, quantity,
                  note)
             values ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(line.order_line_item_id.as_uuid())
        .bind(line.return_reason_id)
        .bind(line.quantity)
        .bind(&line.note)
        .execute(&mut **tx)
        .await?;
    }

    Ok(order_return)
}

/// Points a change at the rows it was opened for. Kept apart from
/// [`request_change`] so the change's shape does not have to know about
/// returns, exchanges and claims all at once.
async fn attach_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change_id: OrderChangeId,
    order_return_id: Option<ReturnId>,
    order_exchange_id: Option<ExchangeId>,
    order_claim_id: Option<ClaimId>,
) -> Result<()> {
    sqlx::query(
        "update order_change
         set order_return_id = $3, order_exchange_id = $4, order_claim_id = $5
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(change_id.as_uuid())
    .bind(order_return_id.map(ReturnId::as_uuid))
    .bind(order_exchange_id.map(ExchangeId::as_uuid))
    .bind(order_claim_id.map(ClaimId::as_uuid))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Puts sellable stock back where the return came in, one inventory item per
/// line, multiplied by what the variant needs of it.
async fn restock(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_return: &Return,
    line_id: LineItemId,
    quantity: i32,
) -> Result<()> {
    let Some(location_id) = order_return.location_id else {
        return Ok(());
    };

    let variant_id: Option<Uuid> =
        sqlx::query_scalar("select variant_id from order_line_item where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(line_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?
            .flatten();

    let Some(variant_id) = variant_id else {
        return Ok(());
    };

    let items =
        crate::inventory::inventory_items_for_variant(tx, ctx, VariantId::from_uuid(variant_id))
            .await?;

    for item in items {
        crate::inventory::adjust_stock(
            tx,
            ctx,
            item.inventory_item_id,
            StockLocationId::from_uuid(location_id),
            quantity * item.required_quantity,
            Some("return received"),
        )
        .await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    line_id: LineItemId,
    version: i32,
    quantity: i32,
    unit_price: Decimal,
    currency: Currency,
    metadata: Value,
) -> Result<()> {
    sqlx::query(
        "insert into order_item
             (id, scope, order_id, order_line_item_id, version, unit_price, currency_code,
              quantity, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(OrderItemId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(line_id.as_uuid())
    .bind(version)
    .bind(unit_price)
    .bind(currency.as_str())
    .bind(quantity)
    .bind(metadata)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn insert_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
    method: &NewOrderShipping,
    currency: Currency,
) -> Result<()> {
    sqlx::query(
        "insert into order_shipping_method
             (id, scope, order_id, version, name, description, shipping_option_id, amount,
              currency_code, is_tax_inclusive, data, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(&method.name)
    .bind(&method.description)
    .bind(method.shipping_option_id.map(ShippingOptionId::as_uuid))
    .bind(method.amount.amount)
    .bind(currency.as_str())
    .bind(method.is_tax_inclusive)
    .bind(&method.data)
    .bind(charges(method.discount, method.tax_rate))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// What the promotions and the tax engine decided, kept on the row so the
/// order can be added up again without either of them.
fn charges(discount: Decimal, tax_rate: Decimal) -> Value {
    serde_json::json!({
        "discount": discount.to_string(),
        "tax_rate": tax_rate.to_string(),
    })
}

async fn bump(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
    line_id: LineItemId,
    column: &str,
    by: i32,
) -> Result<()> {
    if by <= 0 {
        return Err(Error::invalid("that is not a quantity"));
    }

    // The column name is one of a fixed set chosen by this module, never by a
    // caller: there is nothing here for a value to escape into.
    let moved = sqlx::query(&format!(
        "update order_item set {column} = {column} + $4
         where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $5"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(by)
    .bind(line_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict("that line is not on this version"));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn touch(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
    line_id: LineItemId,
    assignment: &str,
    value: i32,
    complaint: &'static str,
) -> Result<()> {
    let moved = sqlx::query(&format!(
        "update order_item set {assignment}
         where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $5"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(value)
    .bind(line_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(complaint));
    }

    Ok(())
}

async fn line_price(tx: &mut Tx<'_>, ctx: &Ctx<'_>, line_id: LineItemId) -> Result<Decimal> {
    sqlx::query_scalar("select unit_price from order_line_item where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(line_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found("line item"))
}

fn quantity_of(details: &Value) -> Result<i32> {
    details
        .get("quantity")
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::invalid("that action carries no quantity"))
}

async fn next_display(tx: &mut Tx<'_>, ctx: &Ctx<'_>, table: &str) -> Result<i64> {
    // `table` is a literal chosen at each call site, never a caller's string.
    let next: i64 = sqlx::query_scalar(&format!(
        "select coalesce(max(display_id), 0) + 1 from {table} where scope = $1"
    ))
    .bind(ctx.scope.0)
    .fetch_one(&mut **tx)
    .await?;

    Ok(next)
}

async fn exponent_of(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<u32> {
    let exponent: Option<i16> =
        sqlx::query_scalar("select exponent from currency where scope = $1 and code = $2")
            .bind(ctx.scope.0)
            .bind(code)
            .fetch_optional(&mut **tx)
            .await?;

    Ok(u32::try_from(exponent.unwrap_or(2)).unwrap_or(2))
}

fn actor_name(ctx: &Ctx<'_>) -> Option<String> {
    match &ctx.actor {
        crate::ports::Actor::Staff { id } | crate::ports::Actor::Customer { id } => {
            Some(id.to_string())
        }
        crate::ports::Actor::Guest { cart } => Some(cart.to_string()),
        crate::ports::Actor::System => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_order_walks_forward_and_stops_at_the_end() {
        assert!(can_transition(OrderStatus::Draft, OrderStatus::Pending));
        assert!(can_transition(OrderStatus::Pending, OrderStatus::Completed));
        assert!(can_transition(
            OrderStatus::Completed,
            OrderStatus::Archived
        ));

        assert!(!can_transition(
            OrderStatus::Completed,
            OrderStatus::Pending
        ));
        assert!(!can_transition(OrderStatus::Canceled, OrderStatus::Pending));
        assert!(!can_transition(
            OrderStatus::Archived,
            OrderStatus::Completed
        ));
        assert!(!can_transition(OrderStatus::Pending, OrderStatus::Draft));
    }

    #[test]
    fn moving_to_the_status_already_held_is_allowed() {
        assert!(can_transition(OrderStatus::Canceled, OrderStatus::Canceled));
    }

    #[test]
    fn a_status_survives_the_round_trip_through_text() -> Result<()> {
        for status in [
            OrderStatus::Draft,
            OrderStatus::Pending,
            OrderStatus::RequiresAction,
            OrderStatus::Completed,
            OrderStatus::Canceled,
            OrderStatus::Archived,
        ] {
            assert_eq!(OrderStatus::parse(status.as_str())?, status);
        }
        Ok(())
    }
}
