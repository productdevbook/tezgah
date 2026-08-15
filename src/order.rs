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
//! up; `order.payment_status` and `order.fulfillment_status` are written by
//! triggers from those same rows, so a back office can filter on a column that
//! no caller is able to set wrong.
//!
//! The ledger settles against what the card is charged rather than against the
//! price tag, because on an instalment sale the two are different numbers and
//! the bank statement is the one money actually moved by. The price tag is
//! what an invoice is issued for, which is why [`Ledger`] carries both.
//!
//! **A document issued about an order is recorded, not produced.** [`OrderInvoice`]
//! holds the number and the authority's identifier so a second request cannot
//! become a second invoice; making the document is the host's.
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
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::cart::{CartTotals, TotalsLine, TotalsShipping, compute};
use crate::error::{Error, Result};
use crate::id::{
    AddressId, AgreementVersionId, CaptureId, ClaimId, CustomerId, ExchangeId, LineItemId,
    OrderAgreementId, OrderBasketId, OrderChangeId, OrderId, OrderInvoiceId, OrderItemId,
    OrderTransactionId, OrderTransferId, PaymentCollectionId, PaymentId, PromotionId, RefundId,
    RegionId, ReturnId, SalesChannelId, SellingPlanId, ShippingOptionId, StockLocationId,
    SubscriptionId, VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Actor, AuditEntry, Ctx, Event, Permit, Resource, Tx};

const ORDER_COLUMNS: &str = "id, display_id, region_id, sales_channel_id, customer_id, \
                             shipping_address_id, billing_address_id, payment_collection_id, \
                             basket_id, subscription_id, email, currency_code, locale, version, \
                             status, payment_status, fulfillment_status, is_draft, \
                             no_notification, metadata, completed_at, canceled_at, created_at, \
                             updated_at";

const LINE_COLUMNS: &str = "id, order_id, variant_id, product_id, title, subtitle, thumbnail, \
                            product_title, product_handle, variant_title, variant_sku, \
                            variant_option_values, unit_price, compare_at_unit_price, \
                            currency_code, requires_shipping, is_tax_inclusive, is_discountable, \
                            is_giftcard, withdrawal_eligible, withdrawal_exclusion_reason, \
                            parent_line_item_id, selling_plan_id, metadata, created_at, updated_at";

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
                              requested_at, received_at, canceled_at, notified_at, \
                              goods_returned_at, refund_due_by, metadata, created_at, \
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
    /// What the card is being charged less what is paid. Negative means
    /// over-refunded.
    pub due: Money,
    /// The instalment difference the shopper agreed to carry on top of the
    /// order — zero for everything that is not an instalment sale.
    pub surcharge: Money,
    /// The order's total plus that difference: what the card is charged, and
    /// what the ledger settles against.
    pub charged: Money,
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
    /// The basket this order was split from at checkout, when it was placed
    /// alongside another seller's. `null` for a single-seller order, forever —
    /// nothing backfills it. Set, `payment_collection_id` is not: which
    /// collection paid is read through the basket instead, so there is one
    /// answer to that question rather than two that can disagree.
    pub basket_id: Option<OrderBasketId>,
    /// The contract this order was placed under, if any — the initial order
    /// a subscription is sold on, or one a renewal placed.
    pub subscription_id: Option<SubscriptionId>,
    pub email: Option<String>,
    pub currency_code: String,
    pub locale: Option<String>,
    pub version: i32,
    pub status: String,
    /// Maintained by the database from `order_transaction`; [`ledger`] is the
    /// arithmetic and this is the same answer, indexable.
    pub payment_status: String,
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
    /// Whether this line could be walked away from, as the rule read on the
    /// day it was bought. Never derived again afterwards.
    pub withdrawal_eligible: bool,
    pub withdrawal_exclusion_reason: Option<String>,
    /// The bundle line this one is a component of, when the order line it
    /// came from was one — carried across from `cart_line_item` the same way
    /// everything else on this row is, at the moment the order is placed.
    pub parent_line_item_id: Option<LineItemId>,
    /// The plan this line was bought on, when it was a subscription's.
    pub selling_plan_id: Option<SellingPlanId>,
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
    /// When the buyer said they were withdrawing, which is what starts the
    /// clock on the refund rather than when the goods arrive back.
    pub notified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub goods_returned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub refund_due_by: Option<chrono::DateTime<chrono::Utc>>,
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

/// What a promotion took off a line or a shipping method, as the order will
/// keep it. The amount is in the order's currency; there is no second one to
/// disagree with it.
#[derive(Debug, Clone)]
pub struct NewAdjustment {
    pub promotion_id: Option<Uuid>,
    pub code: Option<String>,
    pub amount: Decimal,
    pub description: Option<String>,
    pub is_tax_inclusive: bool,
    pub provider_id: Option<String>,
}

impl NewAdjustment {
    pub fn of(amount: Decimal) -> Self {
        NewAdjustment {
            promotion_id: None,
            code: None,
            amount,
            description: None,
            is_tax_inclusive: false,
            provider_id: None,
        }
    }
}

/// One tax rate against one line, kept apart from the others so an invoice can
/// print which rate contributed what and a partial refund can give that same
/// part back. `rate` is a percentage: 18 is eighteen percent.
#[derive(Debug, Clone)]
pub struct NewTaxLine {
    pub rate: Decimal,
    pub code: String,
    pub name: String,
    pub provider_id: Option<String>,
    pub description: Option<String>,
    /// Everything below is the snapshot the order keeps: why this line was
    /// taxed the way it was, which authority the share belongs to, and what the
    /// answer rested on. Copied in rather than joined to, because an OSS record
    /// outlives every table it could join to.
    pub snapshot: TaxSnapshot,
}

/// What was true when the tax was worked out, frozen on the line.
#[derive(Debug, Clone, Default)]
pub struct TaxSnapshot {
    pub treatment: Option<String>,
    pub jurisdiction_level: Option<String>,
    pub jurisdiction_code: Option<String>,
    pub jurisdiction_name: Option<String>,
    pub tax_code: Option<String>,
    pub provider: Option<String>,
    pub provider_transaction_id: Option<String>,
    pub calculated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub address_country_code: Option<String>,
    pub address_province_code: Option<String>,
    pub address_postal_code: Option<String>,
    pub tax_id: Option<String>,
    pub tax_id_evidence: Option<String>,
    pub exemption_id: Option<Uuid>,
    pub evidence: Option<serde_json::Value>,
}

impl NewTaxLine {
    pub fn of(rate: Decimal, code: impl Into<String>, name: impl Into<String>) -> Self {
        NewTaxLine {
            rate,
            code: code.into(),
            name: name.into(),
            provider_id: None,
            description: None,
            snapshot: TaxSnapshot::default(),
        }
    }
}

/// One line as the order will keep it: a snapshot, plus what the promotions
/// and the tax engine had already decided about it.
///
/// The adjustments and tax lines are carried rather than looked up: they are
/// copied into the order's own tables, so an order adds up to the same figure
/// a year later whatever has happened to the promotion or the rate since.
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
    /// This line is a gift card rather than goods. Selling one takes money in
    /// that is not revenue — it is a liability until the card is spent — so it
    /// is not taxed here and `create` refuses a tax line on it. The tax is
    /// charged on whatever the card eventually buys.
    pub is_giftcard: bool,
    pub adjustments: Vec<NewAdjustment>,
    pub tax_lines: Vec<NewTaxLine>,
    /// Why this line is outside the withdrawal right, decided here because the
    /// list of exemptions moves and the answer wanted is the one that held on
    /// the day of the sale. `None` is the ordinary case: it may be sent back.
    pub withdrawal_exclusion: Option<WithdrawalExclusion>,
    /// The cart line whose reservations this line inherits. Without it the
    /// stock a checkout held is unreachable from the order that holds it.
    pub reserved_for: Option<LineItemId>,
    /// The cart line id of the bundle this line's cart line was a component
    /// of, named by its cart line rather than by the order line it becomes —
    /// the order line does not exist yet when the caller builds this. `create`
    /// resolves it to the sibling order line once every line in the same
    /// batch has an id, the same two-pass shape `reserved_for` would need if
    /// a cart line could ever depend on another cart line's new order id.
    pub parent_cart_line: Option<LineItemId>,
    /// The plan this line was bought on, when it was a subscription's.
    /// `checkout`'s `create_subscriptions` step is what a contract is opened
    /// from, reading this off the line rather than the cart it came from.
    pub selling_plan_id: Option<SellingPlanId>,
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
            is_giftcard: false,
            adjustments: Vec::new(),
            tax_lines: Vec::new(),
            withdrawal_exclusion: None,
            reserved_for: None,
            parent_cart_line: None,
            selling_plan_id: None,
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
    pub adjustments: Vec<NewAdjustment>,
    pub tax_lines: Vec<NewTaxLine>,
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
    /// Which basket this order was split from at checkout. Carries no
    /// `payment_collection_id` of its own when this is set — the basket's is
    /// the one that paid.
    pub basket_id: Option<OrderBasketId>,
    pub subscription_id: Option<SubscriptionId>,
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
            basket_id: None,
            subscription_id: None,
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
    if new.basket_id.is_some() && new.payment_collection_id.is_some() {
        return Err(Error::invalid(
            "an order under a basket has no payment collection of its own",
        ));
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
        check_money(&line.adjustments, &line.tax_lines)?;
    }
    for method in &new.shipping {
        if method.amount.is_negative() {
            return Err(Error::invalid("a shipping price cannot be negative"));
        }
        if method.amount.currency != currency {
            return Err(Error::invalid("that price is in another currency"));
        }
        check_money(&method.adjustments, &method.tax_lines)?;
    }

    let shipping_address_id = match new.shipping_address {
        Some(address) => Some(write_address(tx, ctx, new.customer_id, &address).await?),
        None => None,
    };
    let billing_address_id = match new.billing_address {
        Some(address) => Some(write_address(tx, ctx, new.customer_id, &address).await?),
        None => None,
    };

    let display_id = next_display(tx, ctx, "order").await?;

    let status = if draft {
        OrderStatus::Draft
    } else {
        OrderStatus::Pending
    };

    let order = sqlx::query_as::<_, Order>(&format!(
        r#"insert into "order"
               (id, scope, display_id, region_id, sales_channel_id, customer_id,
                shipping_address_id, billing_address_id, payment_collection_id, basket_id,
                subscription_id, email, currency_code, locale, status, is_draft,
                no_notification, metadata)
           values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
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
    .bind(new.basket_id.map(OrderBasketId::as_uuid))
    .bind(new.subscription_id.map(SubscriptionId::as_uuid))
    .bind(new.email.map(|value| value.trim().to_lowercase()))
    .bind(currency.as_str())
    .bind(new.locale)
    .bind(status.as_str())
    .bind(draft)
    .bind(new.no_notification)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    // A bundle's parent line is named in each child's `parent_cart_line` by
    // the cart line it came from, not by the order line it becomes — the
    // order line does not exist until this loop inserts it. `by_cart_line`
    // is filled as each line is inserted and resolved once every line in the
    // batch has an id, the same reason `reserved_for` names a cart line
    // rather than something not yet written.
    let mut by_cart_line: std::collections::HashMap<uuid::Uuid, LineItemId> =
        std::collections::HashMap::new();
    let mut pending_parents: Vec<(LineItemId, LineItemId)> = Vec::new();

    for line in new.lines {
        // A gift card is money changing form, not goods changing hands: the
        // tax is charged on what the card buys, and charging it twice is the
        // one thing this flag exists to stop.
        if line.is_giftcard && !line.tax_lines.is_empty() {
            return Err(Error::invalid(
                "a gift card line is not taxed; the tax belongs on what the card buys",
            ));
        }

        let line_id = LineItemId::new();
        sqlx::query(
            "insert into order_line_item
                 (id, scope, order_id, variant_id, product_id, title, subtitle, thumbnail,
                  product_title, product_handle, variant_title, variant_sku,
                  variant_option_values, unit_price, compare_at_unit_price, currency_code,
                  requires_shipping, is_tax_inclusive, is_discountable, is_giftcard,
                  withdrawal_eligible, withdrawal_exclusion_reason, selling_plan_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16,
                     $17, $18, $19, $20, $21, $22, $23)",
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
        .bind(line.is_giftcard)
        .bind(line.withdrawal_exclusion.is_none())
        .bind(line.withdrawal_exclusion.map(WithdrawalExclusion::as_str))
        .bind(line.selling_plan_id.map(SellingPlanId::as_uuid))
        .execute(&mut **tx)
        .await?;

        insert_line_money(
            tx,
            ctx,
            LineMoney::Line(line_id),
            currency,
            &line.adjustments,
            &line.tax_lines,
        )
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
        )
        .await?;

        if let Some(cart_line) = line.reserved_for {
            crate::inventory::rebind_reservations(tx, ctx, cart_line, line_id).await?;
            by_cart_line.insert(cart_line.as_uuid(), line_id);
        }
        if let Some(parent_cart_line) = line.parent_cart_line {
            pending_parents.push((line_id, parent_cart_line));
        }
    }

    for (child, parent_cart_line) in pending_parents {
        let Some(&parent) = by_cart_line.get(&parent_cart_line.as_uuid()) else {
            continue;
        };
        sqlx::query(
            "update order_line_item set parent_line_item_id = $3
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(child.as_uuid())
        .bind(parent.as_uuid())
        .execute(&mut **tx)
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
    let order = read(tx, ctx, Action::View, id).await?;

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

/// This scope's own orders under one basket. The permit is asked by
/// [`crate::order_basket::orders`], the only caller: a basket id crosses
/// scopes on its own, so what may be asked about one is a question for the
/// basket, not this table.
pub(crate) async fn in_basket(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    basket_id: OrderBasketId,
    paging: Paging,
) -> Result<Page<Order>> {
    let rows = sqlx::query_as::<_, Order>(&format!(
        r#"select {ORDER_COLUMNS} from "order"
           where scope = $1
             and basket_id = $2
             and ($3::timestamptz is null or (created_at, id) > ($3, $4))
           order by created_at, id
           limit $5"#
    ))
    .bind(ctx.scope.0)
    .bind(basket_id.as_uuid())
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
    let order = read(tx, ctx, Action::View, order_id).await?;
    let currency = order.currency()?;
    let exponent = crate::store::exponent(tx, ctx, currency).await?;

    let lines = sqlx::query_as::<_, TotalsLine>(
        "select i.quantity,
                coalesce(i.unit_price, l.unit_price) as unit_price,
                l.is_tax_inclusive,
                coalesce((select sum(a.amount) from order_line_item_adjustment a
                          where a.scope = l.scope and a.order_line_item_id = l.id), 0) as discount,
                coalesce((select sum(t.rate) from order_line_item_tax_line t
                          where t.scope = l.scope and t.order_line_item_id = l.id), 0) as tax_rate
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
        "select s.amount, s.is_tax_inclusive,
                coalesce((select sum(a.amount) from order_shipping_method_adjustment a
                          where a.scope = s.scope and a.order_shipping_method_id = s.id),
                         0) as discount,
                coalesce((select sum(t.rate) from order_shipping_method_tax_line t
                          where t.scope = s.scope and t.order_shipping_method_id = s.id),
                         0) as tax_rate
         from order_shipping_method s
         where s.scope = $1 and s.order_id = $2 and s.version = $3",
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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    if to == OrderStatus::Canceled {
        return unwind(tx, ctx, &order, from).await;
    }

    move_status(tx, ctx, order_id, from, to).await
}

/// Cancelling, which is the operation and not the status: an order gives back
/// everything it is holding on the way out.
///
/// Once a parcel has left the building there is nothing to give back, and the
/// customer is owed a return rather than a cancellation. That is refused here
/// rather than half-done.
pub async fn cancel(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<Order> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let from = order.status()?;
    if from == OrderStatus::Canceled {
        return Ok(order);
    }
    if !can_transition(from, OrderStatus::Canceled) {
        return Err(Error::conflict(format!(
            "an order cannot go from {} to canceled",
            from.as_str()
        )));
    }

    unwind(tx, ctx, &order, from).await
}

/// The undoing itself, with the permit already taken and the move already
/// known to be allowed.
/// Takes the parent order row before anything else the path will touch.
///
/// 0022's triggers make every `order_item`, `order_summary` and
/// `order_transaction` write update `"order"`, so the parent is a lock every
/// child write takes. One order for everybody — order first, then inventory —
/// and a return no longer deadlocks against an edit on the same order.
async fn hold_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<()> {
    sqlx::query(r#"select 1 from "order" where scope = $1 and id = $2 for update"#)
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn unwind(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order: &Order, from: OrderStatus) -> Result<Order> {
    hold_order(tx, ctx, order.id).await?;

    if crate::fulfilment::anything_shipped(tx, ctx, order.id).await? {
        return Err(Error::conflict(
            "that order has shipped, so it is returned rather than cancelled",
        ));
    }

    release_payments(tx, ctx, order).await?;

    crate::fulfilment::cancel_open_fulfillments(tx, ctx, order.id).await?;

    for line in line_items(tx, ctx, order.id).await? {
        crate::inventory::release_line(tx, ctx, line.id).await?;
    }

    release_promotions(tx, ctx, order).await?;

    move_status(tx, ctx, order.id, from, OrderStatus::Canceled).await
}

/// Voids the holds the order's authorisations still carry on a card.
///
/// Money already taken is refused rather than half-given-back: a capture has no
/// compensation, it has a refund, and a refund is somebody's decision about how
/// much — so an order that has been paid is refunded first and cancelled after.
/// A payment whose captures have all been refunded already gave its money back
/// and holds nothing, so it is left as it is.
async fn release_payments(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order: &Order) -> Result<()> {
    let Some(collection) = order.payment_collection_id else {
        return Ok(());
    };

    let held: Decimal = sqlx::query_scalar(
        "select coalesce(sum(c.amount), 0) - coalesce((
                    select sum(r.amount) from refund r
                    join payment p on p.scope = r.scope and p.id = r.payment_id
                    where r.scope = $1 and p.payment_collection_id = $2), 0)
         from capture c
         join payment p on p.scope = c.scope and p.id = c.payment_id
         where c.scope = $1 and p.payment_collection_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(collection.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if held > Decimal::ZERO {
        return Err(Error::conflict(
            "money has been taken against that order; refund it before cancelling",
        ));
    }

    let open: Vec<(Uuid, Option<Decimal>)> = sqlx::query_as(
        "select p.id, (select sum(t.amount) from order_transaction t
                       where t.scope = $1 and t.reference = 'payment'
                         and t.reference_id = p.id)
         from payment p
         where p.scope = $1 and p.payment_collection_id = $2 and p.canceled_at is null
           and not exists (select 1 from capture c
                           where c.scope = $1 and c.payment_id = p.id)
         order by p.created_at, p.id",
    )
    .bind(ctx.scope.0)
    .bind(collection.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    let currency = order.currency()?;

    for (payment, authorized) in open {
        crate::payment::cancel(tx, ctx, PaymentId::from_uuid(payment)).await?;

        if let Some(amount) = authorized.filter(|amount| !amount.is_zero()) {
            record_transaction(
                tx,
                ctx,
                order.id,
                Money::new(-amount, currency),
                "payment_canceled",
                payment,
            )
            .await?;
        }
    }

    Ok(())
}

/// Gives back the uses the checkout claimed. The promotions are read from the
/// order's own adjustment rows — what was actually granted, not the cart that
/// granted it — so an order without a cart, or one whose cart has been swept
/// up, still gives its spend back.
async fn release_promotions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order: &Order) -> Result<()> {
    // What each promotion actually gave away, so a spend budget gets back
    // exactly what it was charged rather than a guess.
    let given: Vec<(Uuid, rust_decimal::Decimal)> = sqlx::query_as(
        "select promotion_id, sum(amount) from (
             select a.promotion_id, a.amount
             from order_line_item_adjustment a
             join order_line_item l on l.scope = a.scope and l.id = a.order_line_item_id
             where a.scope = $1 and l.order_id = $2 and a.promotion_id is not null
             union all
             select a.promotion_id, a.amount
             from order_shipping_method_adjustment a
             join order_shipping_method m on m.scope = a.scope and m.id = a.order_shipping_method_id
             where a.scope = $1 and m.order_id = $2 and a.promotion_id is not null
         ) as gave
         group by promotion_id",
    )
    .bind(ctx.scope.0)
    .bind(order.id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    let currency = Currency::parse(&order.currency_code)?;

    for (id, amount) in given {
        crate::promotion::release(
            tx,
            ctx,
            PromotionId::from_uuid(id),
            order.customer_id,
            Money::new(amount, currency),
        )
        .await?;
    }

    Ok(())
}

async fn move_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    from: OrderStatus,
    to: OrderStatus,
) -> Result<Order> {
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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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

    attach_payment_collection(tx, ctx, order_id, payment_collection_id).await?;

    set_status(tx, ctx, order_id, OrderStatus::Pending).await
}

/// The one place an order's `payment_collection_id` is set. Two things can
/// hold an authorisation before this runs — a checkout that opened the
/// collection with the order, which already asked [`record_authorization`]
/// to write it — and a collection assembled and authorised on its own before
/// anything tied it to an order. This is where the second kind catches up:
/// any payment the collection already holds that has no matching movement in
/// [`ledger`]'s own reference gets one now, so `order::ledger` cannot read an
/// authorisation that `order_transaction` never heard about, whichever order
/// the two events happened in.
pub async fn attach_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    payment_collection_id: PaymentCollectionId,
) -> Result<Order> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    sqlx::query(r#"update "order" set payment_collection_id = $3 where scope = $1 and id = $2"#)
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(payment_collection_id.as_uuid())
        .execute(&mut **tx)
        .await?;

    let currency = order.currency()?;

    let unrecorded: Vec<(Uuid, Decimal)> = sqlx::query_as(
        "select p.id, p.amount
         from payment p
         where p.scope = $1 and p.payment_collection_id = $2 and p.canceled_at is null
           and not exists (
             select 1 from order_transaction t
             where t.scope = $1 and t.order_id = $3 and t.reference = 'payment'
               and t.reference_id = p.id
           )
         order by p.created_at, p.id",
    )
    .bind(ctx.scope.0)
    .bind(payment_collection_id.as_uuid())
    .bind(order_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for (payment_id, amount) in unrecorded {
        record_transaction(
            tx,
            ctx,
            order_id,
            Money::new(amount, currency),
            "payment",
            payment_id,
        )
        .await?;
    }

    read(tx, ctx, Action::Write, order_id).await
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
    let order = read(tx, ctx, Action::Settle, order_id).await?;

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
        return Err(Error::invalid(format!(
            "this order is in {}, not {}",
            order.currency_code, amount.currency
        )));
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

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "order",
            entity_id: order_id.as_uuid(),
            summary: serde_json::json!({
                "amount": amount.amount.to_string(),
                "reference": reference,
            }),
        },
    )
    .await?;

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

/// Puts money a provider actually took into the order's ledger.
///
/// The ledger is written here rather than in `payment` because `order` already
/// depends on `payment`, and the arrow back would be a cycle. `capture` names
/// the capture row, so a redelivered webhook meets the unique index instead of
/// paying the order twice.
pub async fn record_capture(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    collection: PaymentCollectionId,
    capture: CaptureId,
    amount: Money,
) -> Result<Option<OrderTransaction>> {
    record_settlement(tx, ctx, collection, "capture", capture.as_uuid(), amount).await
}

/// The same, for money given back: a negative movement, which is how
/// [`ledger`] reads a refund.
pub async fn record_refund(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    collection: PaymentCollectionId,
    refund: RefundId,
    amount: Money,
) -> Result<Option<OrderTransaction>> {
    let back = Money::new(-amount.amount, amount.currency);
    record_settlement(tx, ctx, collection, "refund", refund.as_uuid(), back).await
}

/// Puts a hold a provider actually granted into the order's ledger, the same
/// way [`record_capture`] and [`record_refund`] do for what came after it.
///
/// Called with nothing to write when the collection is not yet an order's —
/// authorising ahead of checkout finishing, or a collection nobody has
/// attached — is not an error: [`attach_payment_collection`] is what catches
/// an authorisation up once the order does claim it.
pub async fn record_authorization(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    collection: PaymentCollectionId,
    payment: PaymentId,
    amount: Money,
) -> Result<Option<OrderTransaction>> {
    record_settlement(tx, ctx, collection, "payment", payment.as_uuid(), amount).await
}

/// The lookup answers nothing to the caller; the permission is taken by
/// [`record_transaction`] against the order it found.
async fn record_settlement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    collection: PaymentCollectionId,
    reference: &str,
    reference_id: Uuid,
    amount: Money,
) -> Result<Option<OrderTransaction>> {
    let order_id: Option<Uuid> = sqlx::query_scalar(
        r#"select id from "order" where scope = $1 and payment_collection_id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(collection.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some(order_id) = order_id else {
        return Ok(None);
    };

    let written = record_transaction(
        tx,
        ctx,
        OrderId::from_uuid(order_id),
        amount,
        reference,
        reference_id,
    )
    .await?;

    Ok(Some(written))
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

/// The payment state, added up from the ledger. `order.payment_status` holds
/// the same answer for a list to filter on, and a database trigger is what
/// writes it — no caller may, so the two cannot disagree.
pub async fn ledger(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<Ledger> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let order = read(tx, ctx, Action::View, order_id).await?;
    let currency = order.currency()?;
    let totals = add_up(tx, ctx, order_id, order.version).await?;

    let (authorized, captured, refunded): (Decimal, Decimal, Decimal) = sqlx::query_as(
        "select
             coalesce(sum(amount) filter (where reference in ('payment', 'payment_canceled')), 0),
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

    // The ledger settles against what the card is charged, not against the
    // price tag: on an instalment sale the two differ by the vade farkı and a
    // `due` computed from the price tag would read a paid order as overpaid.
    // The price tag is still the number the invoice is issued for, which is
    // why both are returned.
    let surcharge: Decimal = match order.payment_collection_id {
        Some(collection) => sqlx::query_scalar(
            "select surcharge_amount from payment_collection where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(collection.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(Decimal::ZERO),
        None => Decimal::ZERO,
    };
    let charged = owed + surcharge;

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
        due: Money::new(charged - paid, currency),
        surcharge: Money::new(surcharge, currency),
        charged: Money::new(charged, currency),
        state,
    })
}

// ---------------------------------------------------------------------------
// Invoices
// ---------------------------------------------------------------------------

/// The document a tax authority issued about an order.
///
/// tezgah does not make one. Rendering UBL, talking to an integrator or to
/// GİB, producing a PDF and sending it are the host's, and `GOAL.md` says so.
/// What is here is the reference: which document was issued for this sale, so
/// asking twice cannot produce two invoices for one order, and so a credit
/// note has something to name.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderInvoice {
    pub id: OrderInvoiceId,
    pub order_id: OrderId,
    pub order_version: i32,
    pub kind: String,
    /// The human-readable serial the shop or the authority allocated.
    pub number: String,
    /// The authority's own identifier — an ETTN in Turkey, a `DocCode`
    /// elsewhere. A different thing from the serial, and the idempotency key.
    pub external_id: Option<String>,
    pub provider: Option<String>,
    pub status: String,
    pub issued_at: Option<chrono::DateTime<chrono::Utc>>,
    pub document_url: Option<String>,
    pub total_amount: Decimal,
    pub currency_code: String,
    pub replaces_invoice_id: Option<OrderInvoiceId>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl OrderInvoice {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn total(&self) -> Result<Money> {
        Ok(Money::new(self.total_amount, self.currency()?))
    }

    pub fn kind(&self) -> InvoiceKind {
        InvoiceKind::parse(&self.kind)
    }

    pub fn status(&self) -> Result<InvoiceStatus> {
        InvoiceStatus::parse(&self.status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceKind {
    Invoice,
    CreditNote,
}

impl InvoiceKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            InvoiceKind::Invoice => "invoice",
            InvoiceKind::CreditNote => "credit_note",
        }
    }

    pub fn parse(text: &str) -> InvoiceKind {
        match text {
            "credit_note" => InvoiceKind::CreditNote,
            _ => InvoiceKind::Invoice,
        }
    }
}

/// Where the document has got to. The answer from a tax authority arrives
/// after the request, so `requested` is a real state rather than a moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Requested,
    Issued,
    Accepted,
    Rejected,
    Cancelled,
}

impl InvoiceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            InvoiceStatus::Requested => "requested",
            InvoiceStatus::Issued => "issued",
            InvoiceStatus::Accepted => "accepted",
            InvoiceStatus::Rejected => "rejected",
            InvoiceStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(text: &str) -> Result<InvoiceStatus> {
        Ok(match text {
            "requested" => InvoiceStatus::Requested,
            "issued" => InvoiceStatus::Issued,
            "accepted" => InvoiceStatus::Accepted,
            "rejected" => InvoiceStatus::Rejected,
            "cancelled" => InvoiceStatus::Cancelled,
            other => {
                return Err(Error::invalid(format!(
                    "{other:?} is not an invoice status"
                )));
            }
        })
    }
}

/// What the host was given by whoever issued the document.
#[derive(Debug, Clone)]
pub struct NewInvoice {
    pub number: String,
    pub external_id: Option<String>,
    pub provider: Option<String>,
    pub status: InvoiceStatus,
    pub total: Money,
    pub issued_at: Option<chrono::DateTime<chrono::Utc>>,
    pub document_url: Option<String>,
    pub metadata: Option<Value>,
}

const INVOICE_COLUMNS: &str = "id, order_id, order_version, kind, number, external_id, provider, \
                               status, issued_at, document_url, total_amount, currency_code, \
                               replaces_invoice_id, metadata, created_at";

/// Records that an invoice was issued for the order.
///
/// One issued invoice per order per version. A second call is refused whatever
/// serial and whatever identifier from the authority it carries, including at
/// `requested`, before the authority has answered and there is no identifier to
/// key on — which is the stage a retry actually happens at. Correcting an
/// invoice that stands is a credit note; a cancelled or rejected one may be
/// reissued. Unwinding two invoices for one sale is a tax problem, not a
/// software one.
pub async fn record_invoice(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    new: NewInvoice,
) -> Result<OrderInvoice> {
    write_invoice(tx, ctx, order_id, InvoiceKind::Invoice, None, new).await
}

/// Records the document that reverses one already issued.
///
/// The original is read first: a credit note against an invoice that was never
/// issued, or against one that was cancelled, is refused.
pub async fn record_credit_note(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    replaces: OrderInvoiceId,
    new: NewInvoice,
) -> Result<OrderInvoice> {
    let original = invoice(tx, ctx, replaces).await?;

    if original.order_id != order_id {
        return Err(Error::invalid("that invoice belongs to another order"));
    }
    if original.kind() != InvoiceKind::Invoice {
        return Err(Error::invalid("a credit note reverses an invoice"));
    }
    if original.status()? == InvoiceStatus::Cancelled {
        return Err(Error::conflict(
            "that invoice was cancelled, so there is nothing to reverse",
        ));
    }

    write_invoice(
        tx,
        ctx,
        order_id,
        InvoiceKind::CreditNote,
        Some(replaces),
        new,
    )
    .await
}

async fn write_invoice(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    kind: InvoiceKind,
    replaces: Option<OrderInvoiceId>,
    new: NewInvoice,
) -> Result<OrderInvoice> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if new.number.trim().is_empty() {
        return Err(Error::invalid("an invoice carries a number"));
    }
    if new.total.currency.as_str() != order.currency_code {
        return Err(Error::invalid("an invoice is in the order's currency"));
    }
    if new.total.is_negative() {
        return Err(Error::invalid(
            "an invoice is for a positive amount; what reverses one is a credit note",
        ));
    }

    let id = OrderInvoiceId::new();
    let written = sqlx::query_as::<_, OrderInvoice>(&format!(
        "insert into order_invoice (
             id, scope, order_id, order_version, kind, number, external_id, provider,
             status, issued_at, document_url, total_amount, currency_code,
             replaces_invoice_id, metadata
         )
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         on conflict do nothing
         returning {INVOICE_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order.version)
    .bind(kind.as_str())
    .bind(new.number.trim())
    .bind(new.external_id.as_deref())
    .bind(new.provider.as_deref())
    .bind(new.status.as_str())
    .bind(new.issued_at)
    .bind(new.document_url.as_deref())
    .bind(new.total.amount)
    .bind(new.total.currency.as_str())
    .bind(replaces.map(OrderInvoiceId::as_uuid))
    .bind(new.metadata)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(written) = written else {
        if kind == InvoiceKind::Invoice {
            let live: Option<uuid::Uuid> = sqlx::query_scalar(
                "select id from order_invoice
                 where scope = $1 and order_id = $2 and order_version = $3
                   and kind = 'invoice' and status not in ('cancelled', 'rejected')",
            )
            .bind(ctx.scope.0)
            .bind(order_id.as_uuid())
            .bind(order.version)
            .fetch_optional(&mut **tx)
            .await?;

            if live.is_some() {
                return Err(Error::conflict(
                    "this order already has an invoice; what corrects one is a credit note, \
                     not a second invoice",
                ));
            }
        }

        return Err(Error::conflict(
            "that document is already recorded against this order",
        ));
    };

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_invoice",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "order": order_id.to_string(),
                "kind": kind.as_str(),
                "number": written.number,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.invoice_recorded",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({
                "invoice": id.to_string(),
                "kind": kind.as_str(),
                "status": new.status.as_str(),
            }),
        },
    )
    .await?;

    Ok(written)
}

/// Writes down what the authority answered, which arrives after the request.
pub async fn set_invoice_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderInvoiceId,
    status: InvoiceStatus,
) -> Result<OrderInvoice> {
    let existing = invoice(tx, ctx, id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: existing.order_id.as_uuid(),
            customer: None,
        },
    )?;

    let updated = sqlx::query_as::<_, OrderInvoice>(&format!(
        "update order_invoice set status = $3
         where scope = $1 and id = $2
         returning {INVOICE_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(status.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order invoice"))?;

    ctx.emit(
        tx,
        Event {
            name: "order.invoice_status_changed",
            entity_id: updated.order_id.as_uuid(),
            payload: serde_json::json!({
                "invoice": id.to_string(),
                "status": status.as_str(),
            }),
        },
    )
    .await?;

    Ok(updated)
}

/// The lookup answers nothing until the permit is taken, and the permit is
/// taken against the order the row turned out to belong to.
pub async fn invoice(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderInvoiceId) -> Result<OrderInvoice> {
    let found = sqlx::query_as::<_, OrderInvoice>(&format!(
        "select {INVOICE_COLUMNS} from order_invoice where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let found = match found {
        Some(found) => found,
        None => {
            let _: Permit = ctx.permit(
                Action::View,
                Resource::Order {
                    id: id.as_uuid(),
                    customer: None,
                },
            )?;
            return Err(Error::not_found("order invoice"));
        }
    };

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: found.order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(found)
}

pub async fn invoices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderInvoice>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderInvoice>(&format!(
        "select {INVOICE_COLUMNS} from order_invoice
         where scope = $1 and order_id = $2
         order by created_at, id limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    let order = read(tx, ctx, Action::Write, change.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    hold_order(tx, ctx, order.id).await?;

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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    let order = read(tx, ctx, Action::Write, order_return.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    hold_order(tx, ctx, order.id).await?;

    if order_return.status == "received"
        || order_return.status == "canceled"
        || order_return.canceled_at.is_some()
    {
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
                   and received_quantity + $3 <= quantity
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
            return Err(
                line_missing_or(tx, ctx, return_id, line.order_line_item_id, || {
                    Error::conflict("that is more than the return asked for")
                })
                .await,
            );
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
         set status = $3,
             received_at = case when $3 = 'received' then $4 else received_at end,
             goods_returned_at = coalesce(goods_returned_at, $4)
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
    let order = read(tx, ctx, Action::Write, order_return.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if order_return.status == "canceled" || order_return.canceled_at.is_some() {
        return Err(Error::conflict("that return was cancelled"));
    }
    if lines.is_empty() {
        return Err(Error::invalid("nothing was dismissed"));
    }

    // Everything is judged before a change is opened: a refusal that had
    // already opened one leaves the order unable to open another.
    for line in &lines {
        if line.quantity <= 0 {
            return Err(Error::invalid("that is not a quantity dismissed"));
        }
        if !return_line_exists(tx, ctx, return_id, line.order_line_item_id).await? {
            return Err(Error::not_found("return item"));
        }
        if !enough_received(tx, ctx, return_id, line.order_line_item_id, line.quantity).await? {
            return Err(Error::conflict("that is more than the return has taken in"));
        }
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
// What was agreed, and how long there is to change one's mind
// ---------------------------------------------------------------------------

/// Days a consumer has to walk away from a distance sale, counted from
/// delivery rather than from the order.
pub const WITHDRAWAL_DAYS: i64 = 14;

/// Days the seller then has to give the money back, counted from the notice
/// rather than from the goods arriving.
pub const REFUND_DAYS: i64 = 14;

pub const MAX_AGREEMENTS: i64 = 50;

/// Which document was accepted. The text of each is the host's to write;
/// tezgah only keeps which one was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgreementKind {
    /// The prior-information form a distance seller must present before the
    /// order is placed.
    PreContract,
    /// The distance sale contract itself.
    DistanceSale,
    Other,
}

impl AgreementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AgreementKind::PreContract => "pre_contract",
            AgreementKind::DistanceSale => "distance_sale",
            AgreementKind::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "pre_contract" => Ok(AgreementKind::PreContract),
            "distance_sale" => Ok(AgreementKind::DistanceSale),
            "other" => Ok(AgreementKind::Other),
            _ => Err(Error::invalid("that is not a kind of agreement")),
        }
    }
}

/// Why a line is outside the withdrawal right. Which goods fall in the list
/// changes — telephones, tablets and computers returned to the Turkish one on
/// 1 January 2026 — so this is recorded at the sale and never worked out again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WithdrawalExclusion {
    CustomMade,
    Hygiene,
    Perishable,
    DigitalUnsealed,
    DigitalDelivered,
    Periodical,
    ServiceStarted,
    Other,
}

impl WithdrawalExclusion {
    pub fn as_str(self) -> &'static str {
        match self {
            WithdrawalExclusion::CustomMade => "custom_made",
            WithdrawalExclusion::Hygiene => "hygiene",
            WithdrawalExclusion::Perishable => "perishable",
            WithdrawalExclusion::DigitalUnsealed => "digital_unsealed",
            WithdrawalExclusion::DigitalDelivered => "digital_delivered",
            WithdrawalExclusion::Periodical => "periodical",
            WithdrawalExclusion::ServiceStarted => "service_started",
            WithdrawalExclusion::Other => "other",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "custom_made" => Ok(WithdrawalExclusion::CustomMade),
            "hygiene" => Ok(WithdrawalExclusion::Hygiene),
            "perishable" => Ok(WithdrawalExclusion::Perishable),
            "digital_unsealed" => Ok(WithdrawalExclusion::DigitalUnsealed),
            "digital_delivered" => Ok(WithdrawalExclusion::DigitalDelivered),
            "periodical" => Ok(WithdrawalExclusion::Periodical),
            "service_started" => Ok(WithdrawalExclusion::ServiceStarted),
            "other" => Ok(WithdrawalExclusion::Other),
            _ => Err(Error::invalid("that is not a withdrawal exclusion")),
        }
    }
}

/// One rendering of one document, as it read the day it was published.
///
/// `body` is the whole text rather than a key into a template: a template is
/// editable, and editing it would destroy the evidence for every order that
/// pointed at it. Rows here are written once and the database refuses to
/// change them.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AgreementVersion {
    pub id: AgreementVersionId,
    pub kind: String,
    pub locale: String,
    pub body: String,
    pub body_hash: String,
    pub effective_from: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const AGREEMENT_COLUMNS: &str =
    "id, kind, locale, body, body_hash, effective_from, metadata, created_at";

/// That one order's buyer accepted one version, and what was known about them
/// as they did.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderAgreement {
    pub id: OrderAgreementId,
    pub order_id: OrderId,
    pub agreement_version_id: AgreementVersionId,
    pub kind: String,
    pub body_hash: String,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

const ORDER_AGREEMENT_COLUMNS: &str = "id, order_id, agreement_version_id, kind, body_hash, \
                                       accepted_at, ip, user_agent, metadata, created_at";

/// A document to publish. The text arrives rendered: tezgah has no template
/// engine, no translations of its own and no opinion about wording.
#[derive(Debug, Clone)]
pub struct NewAgreement {
    pub kind: AgreementKind,
    pub locale: String,
    pub body: String,
    pub effective_from: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<Value>,
}

/// Writes a version of a document. Publishing again writes another version;
/// nothing edits one that exists.
pub async fn publish_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewAgreement,
) -> Result<AgreementVersion> {
    let _: Permit = ctx.permit(Action::Write, Resource::Store)?;

    if new.body.trim().is_empty() {
        return Err(Error::invalid("an agreement with no text in it"));
    }
    if new.locale.trim().is_empty() {
        return Err(Error::invalid("an agreement needs the language it is in"));
    }

    let id = AgreementVersionId::new();
    let version = sqlx::query_as::<_, AgreementVersion>(&format!(
        "insert into agreement_version
             (id, scope, kind, locale, body, body_hash, effective_from, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning {AGREEMENT_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.kind.as_str())
    .bind(new.locale.trim())
    .bind(&new.body)
    .bind(crate::store::digest(&new.body))
    .bind(new.effective_from.unwrap_or_else(|| ctx.now()))
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "agreement_version",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "kind": new.kind.as_str(),
                "locale": version.locale,
                "hash": version.body_hash,
            }),
        },
    )
    .await?;

    Ok(version)
}

pub async fn agreement_versions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    kind: Option<AgreementKind>,
    paging: Paging,
) -> Result<Page<AgreementVersion>> {
    let _: Permit = ctx.permit(Action::View, Resource::Store)?;

    let rows = sqlx::query_as::<_, AgreementVersion>(&format!(
        "select {AGREEMENT_COLUMNS} from agreement_version
         where scope = $1
           and ($2::text is null or kind = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(kind.map(AgreementKind::as_str))
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

pub async fn agreement_version(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: AgreementVersionId,
) -> Result<AgreementVersion> {
    let _: Permit = ctx.permit(Action::View, Resource::Store)?;

    read_agreement(tx, ctx, id).await
}

/// What the buyer did, and everything about the moment worth keeping.
#[derive(Debug, Clone)]
pub struct Acceptance {
    pub agreement_version_id: AgreementVersionId,
    pub accepted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<Value>,
}

impl Acceptance {
    pub fn of(version: AgreementVersionId) -> Self {
        Acceptance {
            agreement_version_id: version,
            accepted_at: None,
            ip: None,
            user_agent: None,
            metadata: None,
        }
    }
}

/// Records that this order's buyer accepted that version.
///
/// The kind and the hash are copied off the version rather than taken from the
/// caller: what is being proved is that this text was accepted, and a caller
/// free to name the text is not proof of anything.
pub async fn accept_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    acceptance: Acceptance,
) -> Result<OrderAgreement> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let version = read_agreement(tx, ctx, acceptance.agreement_version_id).await?;

    let id = OrderAgreementId::new();
    let accepted = sqlx::query_as::<_, OrderAgreement>(&format!(
        "insert into order_agreement
             (id, scope, order_id, agreement_version_id, kind, body_hash, accepted_at,
              ip, user_agent, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (scope, order_id, kind) do nothing
         returning {ORDER_AGREEMENT_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version.id.as_uuid())
    .bind(&version.kind)
    .bind(&version.body_hash)
    .bind(acceptance.accepted_at.unwrap_or_else(|| ctx.now()))
    .bind(acceptance.ip)
    .bind(acceptance.user_agent)
    .bind(acceptance.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that order has already accepted a document of that kind"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_agreement",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "order": order_id,
                "kind": accepted.kind,
                "hash": accepted.body_hash,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.agreement_accepted",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "kind": accepted.kind }),
        },
    )
    .await?;

    Ok(accepted)
}

pub async fn agreements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderAgreement>> {
    let order = read(tx, ctx, Action::View, order_id).await?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    Ok(sqlx::query_as::<_, OrderAgreement>(&format!(
        "select {ORDER_AGREEMENT_COLUMNS} from order_agreement
         where scope = $1 and order_id = $2
         order by accepted_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_AGREEMENTS)
    .fetch_all(&mut **tx)
    .await?)
}

/// The text this order's buyer actually read, whatever has been published
/// since. This is the answer to the only question a regulator asks.
pub async fn accepted_text(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    kind: AgreementKind,
) -> Result<AgreementVersion> {
    let order = read(tx, ctx, Action::View, order_id).await?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    sqlx::query_as::<_, AgreementVersion>(
        "select v.id, v.kind, v.locale, v.body, v.body_hash, v.effective_from,
                v.metadata, v.created_at
         from agreement_version v
         join order_agreement a
           on a.scope = v.scope and a.agreement_version_id = v.id
         where v.scope = $1 and a.order_id = $2 and a.kind = $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(kind.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("accepted agreement"))
}

/// One line's right to be sent back, and by when.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineWithdrawal {
    pub order_line_item_id: LineItemId,
    pub eligible: bool,
    pub exclusion_reason: Option<String>,
    /// The last delivery of this line, which is what the clock runs from.
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `None` when nothing has been delivered — the clock has not started —
    /// and `None` for a line outside the right, which never had one.
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(FromRow)]
struct WithdrawalRow {
    order_line_item_id: LineItemId,
    withdrawal_eligible: bool,
    withdrawal_exclusion_reason: Option<String>,
    delivered_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// When each line's fourteen days run out.
///
/// Computed, never stored: the clock starts at delivery, a delivery can be
/// corrected, and a column written at checkout would be the one thing that did
/// not move with it.
pub async fn withdrawal_deadline(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<LineWithdrawal>> {
    let order = read(tx, ctx, Action::View, order_id).await?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let rows = sqlx::query_as::<_, WithdrawalRow>(
        "select l.id as order_line_item_id,
                l.withdrawal_eligible,
                l.withdrawal_exclusion_reason,
                (select max(f.delivered_at)
                 from fulfillment f
                 join fulfillment_item fi
                   on fi.scope = f.scope and fi.fulfillment_id = f.id
                 join order_item oi
                   on oi.scope = fi.scope and oi.id = fi.line_item_id
                 where f.scope = l.scope
                   and oi.order_line_item_id = l.id
                   and f.canceled_at is null) as delivered_at
         from order_line_item l
         where l.scope = $1 and l.order_id = $2
         order by l.created_at, l.id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| LineWithdrawal {
            order_line_item_id: row.order_line_item_id,
            eligible: row.withdrawal_eligible,
            exclusion_reason: row.withdrawal_exclusion_reason,
            delivered_at: row.delivered_at,
            deadline: row
                .delivered_at
                .filter(|_| row.withdrawal_eligible)
                .map(|at| at + chrono::Duration::days(WITHDRAWAL_DAYS)),
        })
        .collect())
}

/// The buyer says they are withdrawing.
///
/// This is the moment the money starts being owed: every line on the return is
/// checked against its own deadline first, and the refund is due fourteen days
/// from here rather than from the goods coming back.
pub async fn notify_withdrawal(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
) -> Result<Return> {
    let order_return = read_return(tx, ctx, return_id).await?;
    let order = read(tx, ctx, Action::Write, order_return.order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order.id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if order_return.canceled_at.is_some() || order_return.status == "canceled" {
        return Err(Error::conflict("that return was cancelled"));
    }
    if order_return.notified_at.is_some() {
        return Err(Error::conflict("that withdrawal was already notified"));
    }

    let now = ctx.now();
    let windows = withdrawal_deadline(tx, ctx, order.id).await?;
    let lines = return_items(tx, ctx, return_id).await?;
    if lines.is_empty() {
        return Err(Error::invalid("a withdrawal needs something on it"));
    }

    for line in &lines {
        let window = windows
            .iter()
            .find(|w| w.order_line_item_id == line.order_line_item_id)
            .ok_or_else(|| Error::not_found("line item"))?;

        if !window.eligible {
            return Err(Error::conflict(
                "that line is outside the right of withdrawal",
            ));
        }
        match window.deadline {
            None => {
                return Err(Error::conflict(
                    "nothing on that line has been delivered, so no window has opened",
                ));
            }
            Some(deadline) if deadline < now => {
                return Err(Error::conflict("the withdrawal window has closed"));
            }
            Some(_) => {}
        }
    }

    let notified = sqlx::query_as::<_, Return>(&format!(
        "update order_return
         set notified_at = $3, refund_due_by = $4
         where scope = $1 and id = $2 and notified_at is null
         returning {RETURN_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .bind(now)
    .bind(now + chrono::Duration::days(REFUND_DAYS))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that withdrawal was already notified"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_return",
            entity_id: return_id.as_uuid(),
            summary: serde_json::json!({ "withdrawal": true, "refund_due_by": notified.refund_due_by }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.withdrawal_notified",
            entity_id: order.id.as_uuid(),
            payload: serde_json::json!({
                "return": return_id,
                "refund_due_by": notified.refund_due_by,
            }),
        },
    )
    .await?;

    Ok(notified)
}

async fn read_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: AgreementVersionId,
) -> Result<AgreementVersion> {
    sqlx::query_as::<_, AgreementVersion>(&format!(
        "select {AGREEMENT_COLUMNS} from agreement_version where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("agreement version"))
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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    hold_order(tx, ctx, order.id).await?;

    let what = ChangeAction::parse(&action.action)?;
    let line = action
        .reference_id
        .map(LineItemId::from_uuid)
        .ok_or_else(|| Error::invalid("that action names no line"));

    // An edit that moves a quantity has to move what the warehouse is holding
    // with it, and the only honest measure of that is the row either side.
    let counted = match what {
        ChangeAction::ItemAdd | ChangeAction::ItemUpdate | ChangeAction::ItemRemove => {
            action.reference_id.map(LineItemId::from_uuid)
        }
        _ => None,
    };
    let before = match counted {
        Some(line) => item_quantity(tx, ctx, order.id, version, line).await?,
        None => 0,
    };

    let mut after: Option<i32> = None;

    match what {
        ChangeAction::ItemAdd => {
            let line = line?;
            let quantity = quantity_of(&action.details)?;
            let price = match action.amount {
                Some(amount) => amount,
                None => line_price(tx, ctx, line).await?,
            };

            let added: Option<i32> = sqlx::query_scalar(
                "update order_item set quantity = quantity + $4
                 where scope = $1 and order_id = $2 and version = $3
                   and order_line_item_id = $5
                 returning quantity",
            )
            .bind(ctx.scope.0)
            .bind(order.id.as_uuid())
            .bind(version)
            .bind(quantity)
            .bind(line.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;

            after = match added {
                Some(quantity) => Some(quantity),
                None => {
                    insert_item(tx, ctx, order.id, line, version, quantity, price, currency)
                        .await?;
                    Some(quantity)
                }
            };
        }
        ChangeAction::ItemUpdate => {
            let line = line?;
            let quantity = quantity_of(&action.details)?;
            after = Some(
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
                .await?,
            );
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

            after = Some(0);
        }
        ChangeAction::ReturnItem => {
            bump(
                tx,
                ctx,
                order.id,
                version,
                line?,
                "return_requested_quantity",
                "quantity",
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
                "quantity",
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
                "return_received_quantity",
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
                "quantity",
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
                    adjustments: Vec::new(),
                    tax_lines: Vec::new(),
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

            // The adjustments and tax lines are `on delete restrict` since
            // 0025, so what a promotion gave against this line and what tax
            // was charged on it go deliberately rather than by cascade.
            for table in [
                "order_shipping_method_adjustment",
                "order_shipping_method_tax_line",
            ] {
                sqlx::query(&format!(
                    "delete from {table}
                     where scope = $1 and order_shipping_method_id in (
                         select id from order_shipping_method
                         where scope = $1 and order_id = $2 and version = $3 and name = $4
                     )"
                ))
                .bind(ctx.scope.0)
                .bind(order.id.as_uuid())
                .bind(version)
                .bind(name)
                .execute(&mut **tx)
                .await?;
            }

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

    if let (Some(line), Some(after)) = (counted, after) {
        if before > 0 {
            crate::inventory::rescale_line(tx, ctx, line, before, after).await?;
        }
    }

    Ok(())
}

/// How much of one line an order holds at a version, and none when the line is
/// not on it.
async fn item_quantity(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
    line: LineItemId,
) -> Result<i32> {
    let quantity: Option<i32> = sqlx::query_scalar(
        "select quantity from order_item
         where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $4",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(line.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(quantity.unwrap_or(0))
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
        let copy = Uuid::now_v7();
        sqlx::query(
            "insert into order_shipping_method
                 (id, scope, order_id, version, name, description, shipping_option_id, amount,
                  currency_code, is_tax_inclusive, data, metadata)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        )
        .bind(copy)
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

        sqlx::query(
            "insert into order_shipping_method_adjustment
                 (id, scope, order_shipping_method_id, promotion_id, code, amount, currency_code,
                  description, is_tax_inclusive, provider_id, metadata)
             select gen_random_uuid(), scope, $3, promotion_id, code, amount, currency_code,
                    description, is_tax_inclusive, provider_id, metadata
             from order_shipping_method_adjustment
             where scope = $1 and order_shipping_method_id = $2",
        )
        .bind(ctx.scope.0)
        .bind(method.id)
        .bind(copy)
        .execute(&mut **tx)
        .await?;

        sqlx::query(
            "insert into order_shipping_method_tax_line
                 (id, scope, order_shipping_method_id, rate, code, name, provider_id, description,
                  metadata)
             select gen_random_uuid(), scope, $3, rate, code, name, provider_id, description,
                    metadata
             from order_shipping_method_tax_line
             where scope = $1 and order_shipping_method_id = $2",
        )
        .bind(ctx.scope.0)
        .bind(method.id)
        .bind(copy)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Transfer
// ---------------------------------------------------------------------------

/// An order offered to somebody else.
///
/// Ownership moves and nothing else does, so this is not a [`ChangeType`]: no
/// version is written, no item row is copied, and the money owed is untouched.
#[derive(Debug, Clone, FromRow)]
pub struct OrderTransfer {
    pub id: OrderTransferId,
    pub order_id: OrderId,
    pub from_customer_id: Option<CustomerId>,
    pub to_customer_id: Option<CustomerId>,
    pub to_email: String,
    pub status: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub requested_by: Option<String>,
    pub requested_at: chrono::DateTime<chrono::Utc>,
    pub settled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

const TRANSFER_COLUMNS: &str = "id, order_id, from_customer_id, to_customer_id, to_email, \
                                status, expires_at, requested_by, requested_at, settled_at, \
                                created_at, updated_at";

/// A fresh transfer and the one time its token is readable.
///
/// The token is not stored and cannot be read back: only its hash is kept, so
/// whoever asked has to send it to the recipient now or ask again.
#[derive(Debug, Clone)]
pub struct RequestedTransfer {
    pub transfer: OrderTransfer,
    pub token: String,
}

pub async fn request_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    to_email: String,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> Result<RequestedTransfer> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

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
    if expires_at <= ctx.now() {
        return Err(Error::invalid("a transfer that has already expired"));
    }

    let token = fresh_token(tx).await?;
    let id = OrderTransferId::new();
    let transfer = sqlx::query_as::<_, OrderTransfer>(&format!(
        "insert into order_transfer
             (id, scope, order_id, from_customer_id, to_email, token_hash, status,
              expires_at, requested_by, requested_at)
         values ($1, $2, $3, $4, $5, $6, 'requested', $7, $8, $9)
         on conflict (scope, order_id) where status = 'requested' do nothing
         returning {TRANSFER_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(order.customer_id.map(CustomerId::as_uuid))
    .bind(&to_email)
    .bind(crate::store::digest(&token))
    .bind(expires_at)
    .bind(actor_name(ctx))
    .bind(ctx.now())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that order is already offered to somebody"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_transfer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "order": order_id, "to": to_email }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.transfer_requested",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "transfer": id, "to": to_email }),
        },
    )
    .await?;

    Ok(RequestedTransfer { transfer, token })
}

/// Takes the order over. The token is the claim; the permit is asked about the
/// customer who would end up owning it rather than the one who still does.
pub async fn accept_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    token: &str,
    by: CustomerId,
) -> Result<Order> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: Some(by.as_uuid()),
        },
    )?;

    let open = open_transfer(tx, ctx, order_id, Some(token)).await?;
    if open.expires_at <= ctx.now() {
        return Err(Error::conflict("that transfer has expired"));
    }

    settle(tx, ctx, open.id, "accepted", Some(by)).await?;

    let moved = sqlx::query_as::<_, Order>(&format!(
        r#"update "order" set customer_id = $3
           where scope = $1 and id = $2
           returning {ORDER_COLUMNS}"#
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(by.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order",
            entity_id: order_id.as_uuid(),
            summary: serde_json::json!({
                "transfer": open.id,
                "from": open.from_customer_id,
                "to": by,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "order.transferred",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({
                "transfer": open.id,
                "from": open.from_customer_id,
                "to": by,
            }),
        },
    )
    .await?;

    Ok(moved)
}

/// The recipient says no. Holding the token is what says it is theirs to
/// refuse, so an expired one may still be declined.
pub async fn decline_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    token: &str,
) -> Result<OrderTransfer> {
    let open = open_transfer(tx, ctx, order_id, Some(token)).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: open.to_customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let settled = settle(tx, ctx, open.id, "declined", None).await?;

    ctx.emit(
        tx,
        Event {
            name: "order.transfer_declined",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "transfer": open.id }),
        },
    )
    .await?;

    Ok(settled)
}

/// The owner withdraws the offer. No token: whoever still owns the order is
/// who may take it back.
pub async fn cancel_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<OrderTransfer> {
    let order = read(tx, ctx, Action::Write, order_id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: order.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let open = open_transfer(tx, ctx, order_id, None).await?;
    let settled = settle(tx, ctx, open.id, "canceled", None).await?;

    ctx.emit(
        tx,
        Event {
            name: "order.transfer_canceled",
            entity_id: order_id.as_uuid(),
            payload: serde_json::json!({ "transfer": open.id }),
        },
    )
    .await?;

    Ok(settled)
}

/// The order's one open transfer, and when a token is given, only if it is the
/// one that was issued.
async fn open_transfer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    token: Option<&str>,
) -> Result<OrderTransfer> {
    let transfer = sqlx::query_as::<_, OrderTransfer>(&format!(
        "select {TRANSFER_COLUMNS} from order_transfer
         where scope = $1 and order_id = $2 and status = 'requested'"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let transfer = match transfer {
        Some(transfer) => transfer,
        None => {
            let _: Permit = ctx.permit(
                Action::Write,
                Resource::Order {
                    id: order_id.as_uuid(),
                    customer: None,
                },
            )?;
            return Err(Error::not_found("order transfer"));
        }
    };

    if let Some(token) = token {
        let hash: String = sqlx::query_scalar(
            "select token_hash from order_transfer where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(transfer.id.as_uuid())
        .fetch_one(&mut **tx)
        .await?;

        // Constant time: a comparison that stops at the first wrong byte tells
        // whoever is guessing how much of the token they have right.
        let matches: bool = crate::store::digest(token)
            .as_bytes()
            .ct_eq(hash.as_bytes())
            .into();
        if !matches {
            return Err(Error::denied());
        }
    }

    Ok(transfer)
}

async fn settle(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderTransferId,
    status: &str,
    to_customer: Option<CustomerId>,
) -> Result<OrderTransfer> {
    let settled = sqlx::query_as::<_, OrderTransfer>(&format!(
        "update order_transfer
            set status = $3,
                settled_at = $4,
                to_customer_id = coalesce($5, to_customer_id)
          where scope = $1 and id = $2 and status = 'requested'
          returning {TRANSFER_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(status)
    .bind(ctx.now())
    .bind(to_customer.map(CustomerId::as_uuid))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("that transfer was settled while this was being decided"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_transfer",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "status": status }),
        },
    )
    .await?;

    Ok(settled)
}

/// 256 bits from the database's own generator, so no host has to supply one.
async fn fresh_token(tx: &mut Tx<'_>) -> Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "select replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '')",
    )
    .fetch_one(&mut **tx)
    .await?)
}

// ---------------------------------------------------------------------------
// The small shared parts
// ---------------------------------------------------------------------------

/// Loads the row, or asks whether the caller may even be told it is missing.
///
/// A nonexistent id and one that exists and belongs to somebody else must not
/// answer differently to a caller with no standing to ask either — order ids
/// carry a uuidv7 timestamp, so a distinguishable `not_found` would let
/// anybody probe when this shop's orders were created. So a miss asks
/// `action` against the order before saying so; only a caller that could have
/// been told "no, that is not yours" is told "there is no such order".
async fn read(tx: &mut Tx<'_>, ctx: &Ctx<'_>, action: Action, id: OrderId) -> Result<Order> {
    let order = sqlx::query_as::<_, Order>(&format!(
        r#"select {ORDER_COLUMNS} from "order" where scope = $1 and id = $2"#
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match order {
        Some(order) => Ok(order),
        None => {
            let _: Permit = ctx.permit(
                action,
                Resource::Order {
                    id: id.as_uuid(),
                    customer: None,
                },
            )?;
            Err(Error::not_found("order"))
        }
    }
}

/// Same shape as [`read`], for a change reached by its own id rather than the
/// order's: every caller in this module asks `Action::Write` of the order it
/// resolves to, so a miss asks that in the change's stead.
async fn read_change(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderChangeId) -> Result<OrderChange> {
    let change = sqlx::query_as::<_, OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match change {
        Some(change) => Ok(change),
        None => {
            let _: Permit = ctx.permit(
                Action::Write,
                Resource::Order {
                    id: id.as_uuid(),
                    customer: None,
                },
            )?;
            Err(Error::not_found("order change"))
        }
    }
}

/// Same shape as [`read_change`].
async fn read_return(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReturnId) -> Result<Return> {
    let order_return = sqlx::query_as::<_, Return>(&format!(
        "select {RETURN_COLUMNS} from order_return where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match order_return {
        Some(order_return) => Ok(order_return),
        None => {
            let _: Permit = ctx.permit(
                Action::Write,
                Resource::Order {
                    id: id.as_uuid(),
                    customer: None,
                },
            )?;
            Err(Error::not_found("return"))
        }
    }
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
) -> Result<()> {
    sqlx::query(
        "insert into order_item
             (id, scope, order_id, order_line_item_id, version, unit_price, currency_code,
              quantity)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(OrderItemId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(line_id.as_uuid())
    .bind(version)
    .bind(unit_price)
    .bind(currency.as_str())
    .bind(quantity)
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
) -> Result<Uuid> {
    let id = Uuid::now_v7();

    sqlx::query(
        "insert into order_shipping_method
             (id, scope, order_id, version, name, description, shipping_option_id, amount,
              currency_code, is_tax_inclusive, data)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
    )
    .bind(id)
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
    .execute(&mut **tx)
    .await?;

    insert_line_money(
        tx,
        ctx,
        LineMoney::Shipping(id),
        currency,
        &method.adjustments,
        &method.tax_lines,
    )
    .await?;

    Ok(id)
}

/// Which pair of tables a set of adjustments and tax lines belongs in.
#[derive(Debug, Clone, Copy)]
enum LineMoney {
    Line(LineItemId),
    Shipping(Uuid),
}

impl LineMoney {
    fn tables(self) -> (&'static str, &'static str, &'static str) {
        match self {
            LineMoney::Line(_) => (
                "order_line_item_adjustment",
                "order_line_item_tax_line",
                "order_line_item_id",
            ),
            LineMoney::Shipping(_) => (
                "order_shipping_method_adjustment",
                "order_shipping_method_tax_line",
                "order_shipping_method_id",
            ),
        }
    }

    fn owner(self) -> Uuid {
        match self {
            LineMoney::Line(id) => id.as_uuid(),
            LineMoney::Shipping(id) => id,
        }
    }
}

fn check_money(adjustments: &[NewAdjustment], tax_lines: &[NewTaxLine]) -> Result<()> {
    for adjustment in adjustments {
        if adjustment.amount.is_sign_negative() {
            return Err(Error::invalid("a discount cannot be negative"));
        }
    }
    for tax in tax_lines {
        if tax.rate.is_sign_negative() || tax.rate > Decimal::ONE_HUNDRED {
            return Err(Error::invalid(
                "a tax rate is a percentage between 0 and 100",
            ));
        }
    }

    Ok(())
}

async fn insert_line_money(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    owner: LineMoney,
    currency: Currency,
    adjustments: &[NewAdjustment],
    tax_lines: &[NewTaxLine],
) -> Result<()> {
    let (adjustment_table, tax_table, column) = owner.tables();

    for adjustment in adjustments {
        sqlx::query(&format!(
            "insert into {adjustment_table}
                 (id, scope, {column}, promotion_id, code, amount, currency_code, description,
                  is_tax_inclusive, provider_id)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"
        ))
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(owner.owner())
        .bind(adjustment.promotion_id)
        .bind(&adjustment.code)
        .bind(adjustment.amount)
        .bind(currency.as_str())
        .bind(&adjustment.description)
        .bind(adjustment.is_tax_inclusive)
        .bind(&adjustment.provider_id)
        .execute(&mut **tx)
        .await?;
    }

    for tax in tax_lines {
        let snapshot = &tax.snapshot;
        sqlx::query(&format!(
            "insert into {tax_table}
                 (id, scope, {column}, rate, code, name, provider_id, description,
                  treatment, jurisdiction_level, jurisdiction_code, jurisdiction_name,
                  tax_code, provider, provider_transaction_id, calculated_at,
                  address_country_code, address_province_code, address_postal_code,
                  tax_id, tax_id_evidence, exemption_id, evidence)
             values ($1, $2, $3, $4, $5, $6, $7, $8, coalesce($9, 'standard'), $10, $11, $12,
                     $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)"
        ))
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(owner.owner())
        .bind(tax.rate)
        .bind(&tax.code)
        .bind(&tax.name)
        .bind(&tax.provider_id)
        .bind(&tax.description)
        .bind(&snapshot.treatment)
        .bind(&snapshot.jurisdiction_level)
        .bind(&snapshot.jurisdiction_code)
        .bind(&snapshot.jurisdiction_name)
        .bind(&snapshot.tax_code)
        .bind(&snapshot.provider)
        .bind(&snapshot.provider_transaction_id)
        .bind(snapshot.calculated_at)
        .bind(&snapshot.address_country_code)
        .bind(&snapshot.address_province_code)
        .bind(&snapshot.address_postal_code)
        .bind(&snapshot.tax_id)
        .bind(&snapshot.tax_id_evidence)
        .bind(snapshot.exemption_id)
        .bind(&snapshot.evidence)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Whether a line is on a return at all. A read, and only ever to tell a
/// missing line from a quantity that is too large; what bounds the quantity is
/// the conditional update that writes it.
async fn return_line_exists(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
    line_id: LineItemId,
) -> Result<bool> {
    let found: Option<i32> = sqlx::query_scalar(
        "select 1 from order_return_item
         where scope = $1 and order_return_id = $2 and order_line_item_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .bind(line_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found.is_some())
}

/// Whether enough of a line has come in to turn that much of it away. The
/// binding guard is the conditional update in [`bump`]; this is what turns it
/// into a refusal before an order change is opened.
async fn enough_received(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
    line_id: LineItemId,
    quantity: i32,
) -> Result<bool> {
    let found: Option<i32> = sqlx::query_scalar(
        "select 1 from order_return_item
         where scope = $1 and order_return_id = $2 and order_line_item_id = $3
           and received_quantity >= $4",
    )
    .bind(ctx.scope.0)
    .bind(return_id.as_uuid())
    .bind(line_id.as_uuid())
    .bind(quantity)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found.is_some())
}

/// Only on the failure path, so the happy one stays a single statement.
async fn line_missing_or(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
    line_id: LineItemId,
    otherwise: impl Fn() -> Error,
) -> Error {
    match return_line_exists(tx, ctx, return_id, line_id).await {
        Ok(true) => otherwise(),
        Ok(false) => Error::not_found("return item"),
        Err(err) => err,
    }
}

/// Raises one of an item's counters, never past the counter that bounds it.
///
/// The ceiling is in the `where`, not read first and checked after: a database
/// check constraint underneath would refuse the same write, but as a raw
/// violation rather than the conflict a caller can act on.
#[allow(clippy::too_many_arguments)]
async fn bump(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    version: i32,
    line_id: LineItemId,
    column: &str,
    ceiling: &str,
    by: i32,
) -> Result<()> {
    if by <= 0 {
        return Err(Error::invalid("that is not a quantity"));
    }

    // The column names are a fixed set chosen by this module, never by a
    // caller: there is nothing here for a value to escape into.
    let moved = sqlx::query(&format!(
        "update order_item set {column} = {column} + $4
         where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $5
           and {column} + $4 <= {ceiling}"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(by)
    .bind(line_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    if moved.rows_affected() == 0 {
        return Err(Error::conflict(
            "that line is not on this version, or that is more of it than it has left",
        ));
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
) -> Result<i32> {
    let moved: Option<i32> = sqlx::query_scalar(&format!(
        "update order_item set {assignment}
         where scope = $1 and order_id = $2 and version = $3 and order_line_item_id = $5
         returning quantity"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(version)
    .bind(value)
    .bind(line_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    moved.ok_or_else(|| Error::conflict(complaint))
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

/// The next human-facing number of one kind, taken off the counter's own write.
///
/// `max(display_id) + 1` read the same number twice whenever two checkouts
/// committed together, and the loser met the unique index mid-workflow.
async fn next_display(tx: &mut Tx<'_>, ctx: &Ctx<'_>, kind: &str) -> Result<i64> {
    let next: i64 = sqlx::query_scalar(
        "insert into display_counter (id, scope, kind, next)
         values ($1, $2, $3, 1)
         on conflict (scope, kind) do update set next = display_counter.next + 1
         returning next",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(kind)
    .fetch_one(&mut **tx)
    .await?;

    Ok(next)
}

/// Who to write down as having done something.
///
/// A guest is named by the cart they are holding: it is the only handle they
/// have, and "nobody" in an audit row is worse than a handle that expires.
fn actor_name(ctx: &Ctx<'_>) -> Option<String> {
    match &ctx.actor {
        Actor::Staff { id } | Actor::Customer { id } => Some(id.to_string()),
        Actor::Guest { cart } => Some(cart.to_string()),
        Actor::System => None,
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
