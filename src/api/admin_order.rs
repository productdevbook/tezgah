//! What a back office may ask about orders and the money on them.
//!
//! Two rules shape everything here and are not per-route decisions.
//!
//! **Taking money is not editing an order.** A capture or a refund asks for
//! [`Action::Settle`]; everything else asks for [`Action::Write`]. A role that
//! may correct an address is not thereby a role that may move funds.
//!
//! **A return, an exchange and a claim are all one mechanism.** Each opens an
//! [`order::OrderChange`], hangs actions on it and confirms it. Nothing here
//! moves a quantity its own way, because two mechanisms disagree the first time
//! one of them is fixed.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::credit;
use crate::error::{Error, Result};
use crate::fulfilment;
use crate::id::{
    ClaimId, ExchangeId, FulfillmentId, FulfillmentSetId, InventoryItemId, LineItemId,
    OrderChangeId, OrderId, OrderItemId, PaymentCollectionId, PaymentId, PaymentProviderId,
    PaymentSessionId, PaymentWebhookEventId, RefundReasonId, ReturnId, ServiceZoneId,
    ShippingOptionId, ShippingProfileId, StockLocationId,
};
use crate::money::{Currency, Money};
use crate::order;
use crate::page::{Cursor, Page, Paging};
use crate::payment;
use crate::ports::{Action, Ctx, Permit, Resource, Tx};
use crate::settlement;

use super::credit::StoreCreditView;
use super::{Method, Route, Surface};

// ---------------------------------------------------------------------------
// Shared input
// ---------------------------------------------------------------------------

/// Where a page starts and how long it is. Every list here takes one.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl Listing {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match &self.after {
            Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

/// An amount as it arrives over the wire. Never a float, and the currency is
/// carried with it so nothing has to guess which shop's money this is.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoneyIn {
    pub amount: Decimal,
    pub currency: String,
}

impl MoneyIn {
    fn money(&self) -> Result<Money> {
        Ok(Money::new(self.amount, Currency::parse(&self.currency)?))
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct MoneyView {
    pub amount: Decimal,
    pub currency: String,
}

impl From<Money> for MoneyView {
    fn from(money: Money) -> Self {
        MoneyView {
            amount: money.amount,
            currency: money.currency.as_str().to_owned(),
        }
    }
}

fn amount_view(amount: Decimal, currency: &str) -> MoneyView {
    MoneyView {
        amount,
        currency: currency.to_owned(),
    }
}

/// The permission question for one order. Asking it needs the customer, and
/// the customer needs a read, so the read happens first everywhere below.
fn order_resource(id: OrderId, customer: Option<Uuid>) -> Resource {
    Resource::Order {
        id: id.as_uuid(),
        customer,
    }
}

/// Rows that are the shop's own setup rather than anybody's data: the reason
/// lists, the shipping profiles, the option types. Judged as configuration,
/// the way `payment::providers` already is.
fn config() -> Resource {
    Resource::Pricing
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// An order as a back office sees it. Not `order::Order`: that is a row, it
/// grows a column whenever a migration says so, and it carries the scope.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct OrderView {
    pub id: OrderId,
    pub display_id: Option<i64>,
    pub email: Option<String>,
    pub currency_code: String,
    pub version: i32,
    pub status: String,
    pub payment_status: String,
    pub fulfillment_status: String,
    pub is_draft: bool,
    pub payment_collection_id: Option<PaymentCollectionId>,
    /// The basket this order was split from, if a checkout that spanned more
    /// than one seller-scope opened one.
    pub basket_id: Option<crate::id::OrderBasketId>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Order> for OrderView {
    fn from(row: order::Order) -> Self {
        OrderView {
            id: row.id,
            display_id: row.display_id,
            email: row.email,
            currency_code: row.currency_code,
            version: row.version,
            status: row.status,
            payment_status: row.payment_status,
            fulfillment_status: row.fulfillment_status,
            is_draft: row.is_draft,
            payment_collection_id: row.payment_collection_id,
            basket_id: row.basket_id,
            completed_at: row.completed_at,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LineItemView {
    pub id: LineItemId,
    pub title: String,
    pub variant_sku: Option<String>,
    pub unit_price: MoneyView,
    pub requires_shipping: bool,
}

impl From<order::OrderLineItem> for LineItemView {
    fn from(row: order::OrderLineItem) -> Self {
        LineItemView {
            unit_price: amount_view(row.unit_price, &row.currency_code),
            id: row.id,
            title: row.title,
            variant_sku: row.variant_sku,
            requires_shipping: row.requires_shipping,
        }
    }
}

/// One line at one version, with every quantity that has moved on it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct OrderItemView {
    pub id: OrderItemId,
    pub order_line_item_id: LineItemId,
    pub version: i32,
    pub quantity: i32,
    pub fulfilled_quantity: i32,
    pub shipped_quantity: i32,
    pub delivered_quantity: i32,
    pub return_requested_quantity: i32,
    pub return_received_quantity: i32,
    pub return_dismissed_quantity: i32,
    pub written_off_quantity: i32,
}

impl From<order::OrderItem> for OrderItemView {
    fn from(row: order::OrderItem) -> Self {
        OrderItemView {
            id: row.id,
            order_line_item_id: row.order_line_item_id,
            version: row.version,
            quantity: row.quantity,
            fulfilled_quantity: row.fulfilled_quantity,
            shipped_quantity: row.shipped_quantity,
            delivered_quantity: row.delivered_quantity,
            return_requested_quantity: row.return_requested_quantity,
            return_received_quantity: row.return_received_quantity,
            return_dismissed_quantity: row.return_dismissed_quantity,
            written_off_quantity: row.written_off_quantity,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ShippingMethodView {
    pub id: Uuid,
    pub version: i32,
    pub name: String,
    pub amount: MoneyView,
    pub shipping_option_id: Option<ShippingOptionId>,
}

impl From<order::OrderShippingMethod> for ShippingMethodView {
    fn from(row: order::OrderShippingMethod) -> Self {
        ShippingMethodView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            version: row.version,
            name: row.name,
            shipping_option_id: row.shipping_option_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TotalsView {
    pub subtotal: MoneyView,
    pub discount: MoneyView,
    pub shipping: MoneyView,
    pub tax: MoneyView,
    pub total: MoneyView,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SummaryView {
    pub order_id: OrderId,
    pub version: i32,
    pub currency_code: String,
    pub totals: Value,
}

/// What the money on an order comes to. Worked out from the transactions
/// rather than stored, so it cannot drift from them.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LedgerView {
    pub authorized: MoneyView,
    pub captured: MoneyView,
    pub refunded: MoneyView,
    pub paid: MoneyView,
    pub due: MoneyView,
    pub state: &'static str,
}

impl From<order::Ledger> for LedgerView {
    fn from(ledger: order::Ledger) -> Self {
        LedgerView {
            authorized: ledger.authorized.into(),
            captured: ledger.captured.into(),
            refunded: ledger.refunded.into(),
            paid: ledger.paid.into(),
            due: ledger.due.into(),
            state: ledger.state.as_str(),
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct TransactionView {
    pub id: crate::id::OrderTransactionId,
    pub order_id: OrderId,
    pub version: i32,
    pub amount: MoneyView,
    pub reference: Option<String>,
    pub reference_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::OrderTransaction> for TransactionView {
    fn from(row: order::OrderTransaction) -> Self {
        TransactionView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            order_id: row.order_id,
            version: row.version,
            reference: row.reference,
            reference_id: row.reference_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ChangeView {
    pub id: OrderChangeId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_exchange_id: Option<ExchangeId>,
    pub order_claim_id: Option<ClaimId>,
    pub version: i32,
    pub change_type: String,
    pub status: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::OrderChange> for ChangeView {
    fn from(row: order::OrderChange) -> Self {
        ChangeView {
            id: row.id,
            order_id: row.order_id,
            order_return_id: row.order_return_id,
            order_exchange_id: row.order_exchange_id,
            order_claim_id: row.order_claim_id,
            version: row.version,
            change_type: row.change_type,
            status: row.status,
            description: row.description,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ChangeActionView {
    pub id: Uuid,
    pub order_change_id: Option<OrderChangeId>,
    pub ordering: i32,
    pub action: String,
    pub reference_id: Option<Uuid>,
    pub details: Value,
    pub amount: Option<MoneyView>,
    pub applied: bool,
}

impl From<order::OrderChangeAction> for ChangeActionView {
    fn from(row: order::OrderChangeAction) -> Self {
        let amount = match (row.amount, row.currency_code.as_ref()) {
            (Some(amount), Some(currency)) => Some(amount_view(amount, currency)),
            _ => None,
        };
        ChangeActionView {
            amount,
            id: row.id,
            order_change_id: row.order_change_id,
            ordering: row.ordering,
            action: row.action,
            reference_id: row.reference_id,
            details: row.details,
            applied: row.applied,
        }
    }
}

/// A change with its actions, which is the only shape anybody looks at one in.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ChangeDetailView {
    pub change: ChangeView,
    pub actions: Vec<ChangeActionView>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ReturnView {
    pub id: ReturnId,
    pub order_id: OrderId,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub status: String,
    pub location_id: Option<Uuid>,
    pub refund_amount: Option<MoneyView>,
    pub requested_at: Option<chrono::DateTime<chrono::Utc>>,
    pub received_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Return> for ReturnView {
    fn from(row: order::Return) -> Self {
        ReturnView {
            refund_amount: row
                .refund_amount
                .map(|amount| amount_view(amount, &row.currency_code)),
            id: row.id,
            order_id: row.order_id,
            order_version: row.order_version,
            display_id: row.display_id,
            status: row.status,
            location_id: row.location_id,
            requested_at: row.requested_at,
            received_at: row.received_at,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ReturnItemView {
    pub id: Uuid,
    pub order_line_item_id: LineItemId,
    pub return_reason_id: Option<Uuid>,
    pub quantity: i32,
    pub received_quantity: i32,
    pub damaged_quantity: i32,
    pub note: Option<String>,
}

impl From<order::ReturnItem> for ReturnItemView {
    fn from(row: order::ReturnItem) -> Self {
        ReturnItemView {
            id: row.id,
            order_line_item_id: row.order_line_item_id,
            return_reason_id: row.return_reason_id,
            quantity: row.quantity,
            received_quantity: row.received_quantity,
            damaged_quantity: row.damaged_quantity,
            note: row.note,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ExchangeView {
    pub id: ExchangeId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub difference_due: Option<MoneyView>,
    pub allow_backorder: bool,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Exchange> for ExchangeView {
    fn from(row: order::Exchange) -> Self {
        ExchangeView {
            difference_due: row
                .difference_due
                .map(|amount| amount_view(amount, &row.currency_code)),
            id: row.id,
            order_id: row.order_id,
            order_return_id: row.order_return_id,
            order_version: row.order_version,
            display_id: row.display_id,
            allow_backorder: row.allow_backorder,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ClaimView {
    pub id: ClaimId,
    pub order_id: OrderId,
    pub order_return_id: Option<ReturnId>,
    pub order_version: i32,
    pub display_id: Option<i64>,
    pub claim_type: String,
    pub refund_amount: Option<MoneyView>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::Claim> for ClaimView {
    fn from(row: order::Claim) -> Self {
        ClaimView {
            refund_amount: row
                .refund_amount
                .map(|amount| amount_view(amount, &row.currency_code)),
            id: row.id,
            order_id: row.order_id,
            order_return_id: row.order_return_id,
            order_version: row.order_version,
            display_id: row.display_id,
            claim_type: row.claim_type,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

/// One line of a claim: what was wrong with it, or what is being sent
/// instead — [`order::ClaimLine::images`] is what a support agent judges the
/// damage from.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ClaimItemView {
    pub id: Uuid,
    pub order_claim_id: ClaimId,
    pub order_line_item_id: LineItemId,
    pub reason: Option<String>,
    pub quantity: i32,
    pub is_additional_item: bool,
    pub images: Vec<String>,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::ClaimItem> for ClaimItemView {
    fn from(row: order::ClaimItem) -> Self {
        ClaimItemView {
            images: row
                .images
                .map(|value| serde_json::from_value(value).unwrap_or_default())
                .unwrap_or_default(),
            id: row.id,
            order_claim_id: row.order_claim_id,
            order_line_item_id: row.order_line_item_id,
            reason: row.reason,
            quantity: row.quantity,
            is_additional_item: row.is_additional_item,
            note: row.note,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PaymentView {
    pub id: PaymentId,
    pub payment_collection_id: PaymentCollectionId,
    pub payment_session_id: Option<PaymentSessionId>,
    pub amount: MoneyView,
    pub captured_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::Payment> for PaymentView {
    fn from(row: payment::Payment) -> Self {
        PaymentView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            payment_collection_id: row.payment_collection_id,
            payment_session_id: row.payment_session_id,
            captured_at: row.captured_at,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureView {
    pub id: crate::id::CaptureId,
    pub payment_id: PaymentId,
    pub amount: MoneyView,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::Capture> for CaptureView {
    fn from(row: payment::Capture) -> Self {
        CaptureView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            payment_id: row.payment_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RefundView {
    pub id: crate::id::RefundId,
    pub payment_id: PaymentId,
    pub refund_reason_id: Option<RefundReasonId>,
    pub amount: MoneyView,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::Refund> for RefundView {
    fn from(row: payment::Refund) -> Self {
        RefundView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            payment_id: row.payment_id,
            refund_reason_id: row.refund_reason_id,
            note: row.note,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CollectionView {
    pub id: PaymentCollectionId,
    pub currency_code: String,
    pub amount: Decimal,
    pub authorized_amount: Decimal,
    pub captured_amount: Decimal,
    pub refunded_amount: Decimal,
    pub status: String,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::PaymentCollection> for CollectionView {
    fn from(row: payment::PaymentCollection) -> Self {
        CollectionView {
            authorized_amount: row.authorized(),
            captured_amount: row.captured(),
            refunded_amount: row.refunded(),
            id: row.id,
            currency_code: row.currency_code,
            amount: row.amount,
            status: row.status,
            completed_at: row.completed_at,
            created_at: row.created_at,
        }
    }
}

/// A session without its `data`: that is the provider's, and it has held a
/// client secret before now.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct SessionView {
    pub id: PaymentSessionId,
    pub payment_collection_id: PaymentCollectionId,
    pub amount: MoneyView,
    pub status: String,
    pub authorized_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::PaymentSession> for SessionView {
    fn from(row: payment::PaymentSession) -> Self {
        SessionView {
            amount: amount_view(row.amount, &row.currency_code),
            id: row.id,
            payment_collection_id: row.payment_collection_id,
            status: row.status,
            authorized_at: row.authorized_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ProviderView {
    pub id: Uuid,
    pub code: String,
    pub is_enabled: bool,
}

/// A refund reason or a return reason. The two tables differ by a column name
/// and nothing a caller cares about.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, schemars::JsonSchema)]
pub struct ReasonView {
    pub id: Uuid,
    pub code: String,
    pub label: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FulfillmentView {
    pub id: FulfillmentId,
    pub location_id: Option<StockLocationId>,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub requires_shipping: bool,
    pub packed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub shipped_at: Option<chrono::DateTime<chrono::Utc>>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<fulfilment::Fulfillment> for FulfillmentView {
    fn from(row: fulfilment::Fulfillment) -> Self {
        FulfillmentView {
            id: row.id,
            location_id: row.location_id,
            shipping_option_id: row.shipping_option_id,
            requires_shipping: row.requires_shipping,
            packed_at: row.packed_at,
            shipped_at: row.shipped_at,
            delivered_at: row.delivered_at,
            canceled_at: row.canceled_at,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FulfillmentItemView {
    pub id: Uuid,
    pub title: String,
    pub sku: Option<String>,
    pub quantity: i32,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct LabelView {
    pub id: Uuid,
    pub tracking_number: String,
    pub tracking_url: Option<String>,
    pub label_url: Option<String>,
}

/// A fulfilment with what is in the box and what is on the outside of it.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FulfillmentDetailView {
    pub fulfillment: FulfillmentView,
    pub items: Vec<FulfillmentItemView>,
    pub labels: Vec<LabelView>,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct FulfillmentSetView {
    pub id: FulfillmentSetId,
    pub name: String,
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<fulfilment::FulfillmentSet> for FulfillmentSetView {
    fn from(row: fulfilment::FulfillmentSet) -> Self {
        FulfillmentSetView {
            id: row.id,
            name: row.name,
            kind: row.kind,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ServiceZoneView {
    pub id: ServiceZoneId,
    pub name: String,
    pub fulfillment_set_id: FulfillmentSetId,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct ShippingOptionView {
    pub id: ShippingOptionId,
    pub name: String,
    pub price_type: String,
    pub service_zone_id: ServiceZoneId,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub shipping_option_type_id: Option<Uuid>,
    pub is_return: bool,
    pub enabled_in_store: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<fulfilment::ShippingOption> for ShippingOptionView {
    fn from(row: fulfilment::ShippingOption) -> Self {
        ShippingOptionView {
            id: row.id,
            name: row.name,
            price_type: row.price_type,
            service_zone_id: row.service_zone_id,
            shipping_profile_id: row.shipping_profile_id,
            shipping_option_type_id: row.shipping_option_type_id,
            is_return: row.is_return,
            enabled_in_store: row.enabled_in_store,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ShippingOptionRuleView {
    pub id: Uuid,
    pub shipping_option_id: ShippingOptionId,
    pub attribute: String,
    pub operator: String,
    pub value: Option<Value>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, schemars::JsonSchema)]
pub struct ShippingProfileView {
    pub id: ShippingProfileId,
    pub name: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, schemars::JsonSchema)]
pub struct ShippingOptionTypeView {
    pub id: Uuid,
    pub label: String,
    pub code: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Reads the domain does not offer
// ---------------------------------------------------------------------------

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

const CHANGE_COLUMNS: &str = "id, order_id, order_return_id, order_exchange_id, order_claim_id, \
                              version, change_type, status, description, internal_note, \
                              created_by, requested_by, requested_at, confirmed_by, confirmed_at, \
                              declined_by, declined_at, declined_reason, metadata, created_at, \
                              updated_at";

const PAYMENT_COLUMNS: &str = "id, payment_collection_id, payment_session_id, \
                               payment_provider_id, currency_code, amount, data, captured_at, \
                               canceled_at, created_at";

/// Which of a change's three possible parents is being asked about. A literal
/// from this enum reaches the query; nothing a caller sent ever does.
#[derive(Debug, Clone, Copy)]
enum Parent {
    Return,
    Exchange,
    Claim,
}

impl Parent {
    const fn column(self) -> &'static str {
        match self {
            Parent::Return => "order_return_id",
            Parent::Exchange => "order_exchange_id",
            Parent::Claim => "order_claim_id",
        }
    }

    const fn entity(self) -> &'static str {
        match self {
            Parent::Return => "return",
            Parent::Exchange => "exchange",
            Parent::Claim => "claim",
        }
    }
}

/// A miss on any of these asks `Action::View` of the order before saying so —
/// the id in hand is the row's own, not the order's, since the row was never
/// found; see [`Resource::Order`]'s doc on what `customer: None` means there.
async fn read_return(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReturnId) -> Result<order::Return> {
    let row = sqlx::query_as::<_, order::Return>(&format!(
        "select {RETURN_COLUMNS} from order_return where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match row {
        Some(row) => Ok(row),
        None => {
            let _: Permit = ctx.permit(
                Action::View,
                order_resource(OrderId::from_uuid(id.as_uuid()), None),
            )?;
            Err(Error::not_found("return"))
        }
    }
}

async fn read_exchange(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ExchangeId) -> Result<order::Exchange> {
    let row = sqlx::query_as::<_, order::Exchange>(&format!(
        "select {EXCHANGE_COLUMNS} from order_exchange where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match row {
        Some(row) => Ok(row),
        None => {
            let _: Permit = ctx.permit(
                Action::View,
                order_resource(OrderId::from_uuid(id.as_uuid()), None),
            )?;
            Err(Error::not_found("exchange"))
        }
    }
}

async fn read_claim(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ClaimId) -> Result<order::Claim> {
    let row = sqlx::query_as::<_, order::Claim>(&format!(
        "select {CLAIM_COLUMNS} from order_claim where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match row {
        Some(row) => Ok(row),
        None => {
            let _: Permit = ctx.permit(
                Action::View,
                order_resource(OrderId::from_uuid(id.as_uuid()), None),
            )?;
            Err(Error::not_found("claim"))
        }
    }
}

async fn read_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
) -> Result<order::OrderChange> {
    let row = sqlx::query_as::<_, order::OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match row {
        Some(row) => Ok(row),
        None => {
            let _: Permit = ctx.permit(
                Action::View,
                order_resource(OrderId::from_uuid(id.as_uuid()), None),
            )?;
            Err(Error::not_found("order change"))
        }
    }
}

/// The one change still open against a return, an exchange or a claim.
///
/// A partial unique index keeps there from being two, so this is the change
/// that further actions belong on.
async fn open_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    parent: Parent,
    id: Uuid,
) -> Result<order::OrderChange> {
    let change = sqlx::query_as::<_, order::OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change
         where scope = $1 and {} = $2 and status in ('pending', 'requested')
         order by created_at desc
         limit 1",
        parent.column()
    ))
    .bind(ctx.scope.0)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;

    match change {
        Some(change) => Ok(change),
        None => {
            let _: Permit =
                ctx.permit(Action::View, order_resource(OrderId::from_uuid(id), None))?;
            Err(Error::conflict(format!(
                "that {} has no change open on it",
                parent.entity()
            )))
        }
    }
}

/// The change open against an order that has no return, exchange or claim
/// behind it: an edit.
async fn open_edit(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<order::OrderChange> {
    let change = sqlx::query_as::<_, order::OrderChange>(&format!(
        "select {CHANGE_COLUMNS} from order_change
         where scope = $1 and order_id = $2 and change_type = 'edit'
           and status in ('pending', 'requested')
         order by created_at desc
         limit 1"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    match change {
        Some(change) => Ok(change),
        None => {
            let _: Permit = ctx.permit(Action::View, order_resource(id, None))?;
            Err(Error::conflict("that order has no edit open on it"))
        }
    }
}

/// Takes an action off a change that has not been settled yet.
///
/// An applied action is not removed: the quantity it moved is already in the
/// next version, and deleting the row would leave nothing saying why.
async fn drop_action(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    change: OrderChangeId,
    id: Uuid,
) -> Result<()> {
    let removed = sqlx::query(
        "delete from order_change_action
         where scope = $1 and order_change_id = $2 and id = $3 and not applied",
    )
    .bind(ctx.scope.0)
    .bind(change.as_uuid())
    .bind(id)
    .execute(&mut **tx)
    .await?;

    if removed.rows_affected() == 0 {
        return Err(Error::conflict("that action is gone or already applied"));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Orders
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListOrders {
    pub after: Option<String>,
    pub limit: Option<u32>,
    /// What a back office typed into its search box, matched against the
    /// e-mail on an order and its display number. Blank is not a search.
    pub q: Option<String>,
    /// Which end first. Left out, this surface answers newest-first: an
    /// operator opening Orders wants today's, not the first the shop took.
    pub order: Option<crate::page::Order>,
    /// Which column. `created` is the default; `email` is the other one.
    pub by: Option<crate::page::By>,
    pub customer_id: Option<crate::id::CustomerId>,
    /// Asks how many orders match, as well as this page of them. Off unless
    /// asked: it is a second query over the whole match.
    pub count: Option<bool>,
}

impl ListOrders {
    fn listing(&self) -> Listing {
        Listing {
            after: self.after.clone(),
            limit: self.limit,
        }
    }

    fn paging(&self) -> Result<Paging> {
        let paging = self.listing().paging()?;
        Ok(if self.count.unwrap_or(false) {
            paging.counting()
        } else {
            paging
        })
    }
}

/// Everything but the drafts: those have their own list, because a back office
/// looking at orders is not looking at quotes.
pub async fn list_orders(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListOrders,
) -> Result<Page<OrderView>> {
    let page = order::list(
        tx,
        ctx,
        order::OrderFilter {
            customer: query.customer_id,
            drafts: Some(false),
            search: query.q.as_deref().and_then(crate::page::Search::new),
            order: query.order.unwrap_or(crate::page::Order::Newest),
            by: query.by.unwrap_or_default(),
        },
        query.paging()?,
    )
    .await?;

    Ok(page.map(OrderView::from))
}

pub async fn get_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    Ok(order::get(tx, ctx, id).await?.into())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewLineIn {
    pub variant_id: Option<crate::id::VariantId>,
    pub product_id: Option<Uuid>,
    pub title: String,
    pub quantity: i32,
    pub unit_price: MoneyIn,
    #[serde(default)]
    pub requires_shipping: bool,
    #[serde(default)]
    pub is_tax_inclusive: bool,
    #[serde(default)]
    pub discount: Decimal,
    #[serde(default)]
    pub tax_rate: Decimal,
    /// Why this line is outside the right of withdrawal. A draft order is
    /// written by staff who know what they are selling, so it is said here
    /// rather than looked up.
    pub withdrawal_exclusion: Option<String>,
    /// Whether this line sells a gift card. Said here for the same reason:
    /// a draft order's line need name no variant to look the answer up on.
    #[serde(default)]
    pub is_giftcard: bool,
}

impl NewLineIn {
    fn line(self) -> Result<order::NewOrderLine> {
        let exclusion = self
            .withdrawal_exclusion
            .as_deref()
            .map(order::WithdrawalExclusion::parse)
            .transpose()?;

        Ok(order::NewOrderLine {
            selling_plan_id: None,
            variant_id: self.variant_id,
            product_id: self.product_id,
            title: self.title,
            subtitle: None,
            thumbnail: None,
            product_title: None,
            product_handle: None,
            variant_title: None,
            variant_sku: None,
            variant_option_values: None,
            quantity: self.quantity,
            unit_price: self.unit_price.money()?,
            compare_at_unit_price: None,
            is_tax_inclusive: self.is_tax_inclusive,
            is_discountable: true,
            requires_shipping: self.requires_shipping,
            is_giftcard: self.is_giftcard,
            adjustments: charged(self.discount),
            tax_lines: taxed(self.tax_rate),
            withdrawal_exclusion: exclusion,
            reserved_for: None,
            parent_cart_line: None,
            held_reservations: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewShippingIn {
    pub name: String,
    pub amount: MoneyIn,
    pub shipping_option_id: Option<ShippingOptionId>,
    #[serde(default)]
    pub is_tax_inclusive: bool,
    #[serde(default)]
    pub discount: Decimal,
    #[serde(default)]
    pub tax_rate: Decimal,
}

impl NewShippingIn {
    fn shipping(self) -> Result<order::NewOrderShipping> {
        Ok(order::NewOrderShipping {
            name: self.name,
            description: None,
            shipping_option_id: self.shipping_option_id,
            amount: self.amount.money()?,
            is_tax_inclusive: self.is_tax_inclusive,
            data: None,
            adjustments: charged(self.discount),
            tax_lines: taxed(self.tax_rate),
        })
    }
}

/// The HTTP body still sends one figure each. Zero is no row at all, so an
/// undiscounted line carries no adjustment rather than an adjustment of
/// nothing.
fn charged(discount: Decimal) -> Vec<order::NewAdjustment> {
    if discount.is_zero() {
        return Vec::new();
    }
    vec![order::NewAdjustment::of(discount)]
}

fn taxed(rate: Decimal) -> Vec<order::NewTaxLine> {
    if rate.is_zero() {
        return Vec::new();
    }
    vec![order::NewTaxLine::of(rate, "standard", "Standard")]
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateOrder {
    pub currency: String,
    pub email: Option<String>,
    pub customer_id: Option<crate::id::CustomerId>,
    pub region_id: Option<crate::id::RegionId>,
    pub sales_channel_id: Option<crate::id::SalesChannelId>,
    pub locale: Option<String>,
    pub lines: Vec<NewLineIn>,
    #[serde(default)]
    pub shipping: Vec<NewShippingIn>,
    pub metadata: Option<Value>,
}

impl CreateOrder {
    fn new_order(self) -> Result<order::NewOrder> {
        let mut lines = Vec::with_capacity(self.lines.len());
        for line in self.lines {
            lines.push(line.line()?);
        }
        let mut shipping = Vec::with_capacity(self.shipping.len());
        for one in self.shipping {
            shipping.push(one.shipping()?);
        }

        Ok(order::NewOrder {
            region_id: self.region_id,
            sales_channel_id: self.sales_channel_id,
            customer_id: self.customer_id,
            email: self.email,
            currency_code: Currency::parse(&self.currency)?,
            locale: self.locale,
            payment_collection_id: None,
            basket_id: None,
            subscription_id: None,
            shipping_address: None,
            billing_address: None,
            lines,
            shipping,
            no_notification: None,
            metadata: self.metadata,
        })
    }
}

pub async fn create_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, input: CreateOrder) -> Result<OrderView> {
    Ok(order::create(tx, ctx, input.new_order()?).await?.into())
}

/// Moves an order to a status, refusing a move the state machine forbids.
///
/// `order::set_status` is the one place that decides; a route that wrote the
/// column itself would be a second state machine.
async fn move_order(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    to: order::OrderStatus,
) -> Result<OrderView> {
    Ok(order::set_status(tx, ctx, id, to).await?.into())
}

pub async fn complete_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    move_order(tx, ctx, id, order::OrderStatus::Completed).await
}

/// Cancelling is an operation rather than a status: `order::cancel` gives back
/// the stock and the authorisation, and calling it twice is a no-op.
pub async fn cancel_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    Ok(order::cancel(tx, ctx, id).await?.into())
}

pub async fn archive_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    move_order(tx, ctx, id, order::OrderStatus::Archived).await
}

#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressIn {
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

impl AddressIn {
    fn address(self) -> order::OrderAddress {
        order::OrderAddress {
            company: self.company,
            first_name: self.first_name,
            last_name: self.last_name,
            address_1: self.address_1,
            address_2: self.address_2,
            city: self.city,
            province: self.province,
            postal_code: self.postal_code,
            country_code: self.country_code,
            phone: self.phone,
        }
    }
}

/// Corrects the address a parcel is sent to. `order::update_address` is
/// where a shipment already on its way refuses this.
pub async fn update_order_shipping_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: AddressIn,
) -> Result<OrderView> {
    Ok(
        order::update_address(tx, ctx, id, order::AddressKind::Shipping, &input.address())
            .await?
            .into(),
    )
}

/// Corrects the order's billing address. Nothing physical follows it, so
/// unlike the shipping address this is never refused for having shipped.
pub async fn update_order_billing_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: AddressIn,
) -> Result<OrderView> {
    Ok(
        order::update_address(tx, ctx, id, order::AddressKind::Billing, &input.address())
            .await?
            .into(),
    )
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateEmail {
    pub email: String,
}

/// Corrects the address a confirmation, an invoice or a shipping notice is
/// sent to.
pub async fn update_order_email(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: UpdateEmail,
) -> Result<OrderView> {
    Ok(order::update_email(tx, ctx, id, &input.email).await?.into())
}

pub async fn order_line_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
) -> Result<Vec<LineItemView>> {
    let rows = order::line_items(tx, ctx, id).await?;
    Ok(rows.into_iter().map(LineItemView::from).collect())
}

/// The lines at one version. The version is the caller's because an order that
/// has been edited has more than one, and the old one is what a refund is
/// argued from.
pub async fn order_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    version: Option<i32>,
) -> Result<Vec<OrderItemView>> {
    let version = match version {
        Some(version) => version,
        None => order::get(tx, ctx, id).await?.version,
    };
    let rows = order::items(tx, ctx, id, version).await?;
    Ok(rows.into_iter().map(OrderItemView::from).collect())
}

pub async fn order_shipping_methods(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    version: Option<i32>,
) -> Result<Vec<ShippingMethodView>> {
    let version = match version {
        Some(version) => version,
        None => order::get(tx, ctx, id).await?.version,
    };
    let rows = order::shipping_methods(tx, ctx, id, version).await?;
    Ok(rows.into_iter().map(ShippingMethodView::from).collect())
}

pub async fn order_summary(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    version: Option<i32>,
) -> Result<SummaryView> {
    let version = match version {
        Some(version) => version,
        None => order::get(tx, ctx, id).await?.version,
    };
    let row = order::summary(tx, ctx, id, version).await?;
    Ok(SummaryView {
        order_id: row.order_id,
        version: row.version,
        currency_code: row.currency_code,
        totals: row.totals,
    })
}

pub async fn order_totals(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    version: Option<i32>,
) -> Result<TotalsView> {
    let version = match version {
        Some(version) => version,
        None => order::get(tx, ctx, id).await?.version,
    };
    let totals = order::totals(tx, ctx, id, version).await?;
    Ok(TotalsView {
        subtotal: totals.subtotal.into(),
        discount: totals.discount.into(),
        shipping: totals.shipping.into(),
        tax: totals.tax.into(),
        total: totals.total.into(),
    })
}

pub async fn order_ledger(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<LedgerView> {
    Ok(order::ledger(tx, ctx, id).await?.into())
}

pub async fn order_transactions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
) -> Result<Vec<TransactionView>> {
    let rows = order::transactions(tx, ctx, id).await?;
    Ok(rows.into_iter().map(TransactionView::from).collect())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordTransaction {
    pub amount: MoneyIn,
    pub reference: String,
    pub reference_id: Uuid,
}

/// Writes down that money moved outside tezgah — a bank transfer, a cash sale.
/// [`Action::Settle`], asked by `order::record_transaction`, because the
/// ledger is what a refund is measured against.
pub async fn record_transaction(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: RecordTransaction,
) -> Result<TransactionView> {
    let row = order::record_transaction(
        tx,
        ctx,
        id,
        input.amount.money()?,
        &input.reference,
        input.reference_id,
    )
    .await?;
    Ok(row.into())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachPaymentCollection {
    pub payment_collection_id: PaymentCollectionId,
}

/// Puts a payment collection under a placed order — a collection opened and
/// authorised on its own, offline or through another instrument, made this
/// order's the way `convert_draft_order` does for a draft. Any hold the
/// collection already carries is folded into the ledger the same moment, so
/// an authorisation that happened first is not stranded behind an order that
/// only just learned about it.
pub async fn attach_order_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: AttachPaymentCollection,
) -> Result<OrderView> {
    Ok(
        order::attach_payment_collection(tx, ctx, id, input.payment_collection_id)
            .await?
            .into(),
    )
}

pub async fn order_changes(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    query: Listing,
) -> Result<Page<ChangeView>> {
    let page = order::changes(tx, ctx, id, query.paging()?).await?;
    Ok(page.map(ChangeView::from))
}

pub async fn order_returns(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    query: Listing,
) -> Result<Page<ReturnView>> {
    let page = order::returns(tx, ctx, id, query.paging()?).await?;
    Ok(page.map(ReturnView::from))
}

// ---------------------------------------------------------------------------
// Draft orders
// ---------------------------------------------------------------------------

pub async fn list_draft_orders(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListOrders,
) -> Result<Page<OrderView>> {
    let page = order::list(
        tx,
        ctx,
        order::OrderFilter {
            customer: query.customer_id,
            drafts: Some(true),
            search: query.q.as_deref().and_then(crate::page::Search::new),
            order: query.order.unwrap_or(crate::page::Order::Newest),
            by: query.by.unwrap_or_default(),
        },
        query.listing().paging()?,
    )
    .await?;

    Ok(page.map(OrderView::from))
}

pub async fn get_draft_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    let row = order::get(tx, ctx, id).await?;
    if !row.is_draft {
        return Err(Error::not_found("draft order"));
    }
    Ok(row.into())
}

pub async fn create_draft_order(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateOrder,
) -> Result<OrderView> {
    Ok(order::create_draft(tx, ctx, input.new_order()?)
        .await?
        .into())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConvertDraft {
    pub payment_collection_id: PaymentCollectionId,
}

/// A draft leaves the back office. Nothing is re-priced: the summary drawn up
/// with it is the one it goes out with.
pub async fn convert_draft_order(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: ConvertDraft,
) -> Result<OrderView> {
    Ok(
        order::send_draft_for_payment(tx, ctx, id, input.payment_collection_id)
            .await?
            .into(),
    )
}

pub async fn cancel_draft_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    let row = order::get(tx, ctx, id).await?;
    if !row.is_draft {
        return Err(Error::conflict("that order is not a draft"));
    }
    move_order(tx, ctx, id, order::OrderStatus::Canceled).await
}

// ---------------------------------------------------------------------------
// Edits — the one mechanism, wearing four names
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenEdit {
    pub description: Option<String>,
}

pub async fn open_order_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: OpenEdit,
) -> Result<ChangeView> {
    let change =
        order::request_change(tx, ctx, id, order::ChangeType::Edit, input.description).await?;
    Ok(change.into())
}

pub async fn list_order_edits(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    query: Listing,
) -> Result<Page<ChangeView>> {
    let page = order::changes(tx, ctx, id, query.paging()?).await?;
    Ok(Page {
        items: page
            .items
            .into_iter()
            .filter(|row| row.change_type == order::ChangeType::Edit.as_str())
            .map(ChangeView::from)
            .collect(),
        next: page.next,
        // Not `page.total`: the count is of rows before this filter
        // dropped some of them, so it would not describe what is here.
        total: None,
    })
}

pub async fn get_order_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
) -> Result<ChangeDetailView> {
    get_order_change(tx, ctx, id).await
}

/// One change and its actions. The only shape a change is ever looked at in,
/// because a change without its actions says nothing.
pub async fn get_order_change(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
) -> Result<ChangeDetailView> {
    let _: Permit = ctx.permit(
        Action::View,
        order_resource(OrderId::from_uuid(Uuid::nil()), None),
    )?;

    let change = read_change(tx, ctx, id).await?;
    let actions = order::change_actions(tx, ctx, id).await?;

    Ok(ChangeDetailView {
        change: change.into(),
        actions: actions.into_iter().map(ChangeActionView::from).collect(),
    })
}

/// What a caller may ask a change to do to a line.
#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemAction {
    Add,
    Update,
    Remove,
    WriteOff,
}

impl ItemAction {
    fn change_action(self) -> order::ChangeAction {
        match self {
            ItemAction::Add => order::ChangeAction::ItemAdd,
            ItemAction::Update => order::ChangeAction::ItemUpdate,
            ItemAction::Remove => order::ChangeAction::ItemRemove,
            ItemAction::WriteOff => order::ChangeAction::WriteOffItem,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddItemAction {
    pub action: ItemAction,
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub unit_price: Option<MoneyIn>,
    pub internal_note: Option<String>,
}

impl AddItemAction {
    fn new_action(self) -> Result<order::NewAction> {
        let mut details = serde_json::json!({ "quantity": self.quantity });
        let amount = match &self.unit_price {
            Some(price) => {
                let money = price.money()?;
                if let Some(map) = details.as_object_mut() {
                    map.insert("unit_price".into(), Value::String(money.amount.to_string()));
                    map.insert(
                        "currency_code".into(),
                        Value::String(money.currency.as_str().to_owned()),
                    );
                }
                Some(money)
            }
            None => None,
        };

        Ok(order::NewAction {
            action: self.action.change_action(),
            order_line_item_id: Some(self.order_line_item_id),
            details,
            amount,
            internal_note: self.internal_note,
        })
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddShippingAction {
    pub name: String,
    pub amount: MoneyIn,
    pub internal_note: Option<String>,
}

impl AddShippingAction {
    fn new_action(self) -> Result<order::NewAction> {
        let money = self.amount.money()?;
        Ok(order::NewAction {
            action: order::ChangeAction::ShippingAdd,
            order_line_item_id: None,
            details: serde_json::json!({
                "name": self.name,
                "amount": money.amount.to_string(),
                "currency_code": money.currency.as_str(),
            }),
            amount: Some(money),
            internal_note: self.internal_note,
        })
    }
}

pub async fn add_order_edit_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
    input: AddItemAction,
) -> Result<ChangeActionView> {
    Ok(order::add_action(tx, ctx, id, input.new_action()?)
        .await?
        .into())
}

pub async fn remove_order_edit_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
    action_id: Uuid,
) -> Result<()> {
    let change = read_change(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::Write, order_resource(change.order_id, None))?;
    drop_action(tx, ctx, id, action_id).await
}

pub async fn add_order_edit_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    Ok(order::add_action(tx, ctx, id, input.new_action()?)
        .await?
        .into())
}

pub async fn remove_order_edit_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
    action_id: Uuid,
) -> Result<()> {
    remove_order_edit_item(tx, ctx, id, action_id).await
}

/// Carries the change into the order's next version. Everything it does lands
/// in the caller's transaction, so a failure halfway leaves no half-edit.
pub async fn confirm_order_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
) -> Result<OrderView> {
    Ok(order::confirm_change(tx, ctx, id).await?.into())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclineChange {
    pub reason: Option<String>,
}

pub async fn decline_order_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderChangeId,
    input: DeclineChange,
) -> Result<ChangeView> {
    Ok(order::decline_change(tx, ctx, id, input.reason)
        .await?
        .into())
}

// The same edit, reached through a draft order's own paths.

pub async fn open_draft_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: OpenEdit,
) -> Result<ChangeView> {
    open_order_edit(tx, ctx, id, input).await
}

pub async fn get_draft_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
) -> Result<ChangeDetailView> {
    let change = open_edit(tx, ctx, id).await?;
    get_order_change(tx, ctx, change.id).await
}

pub async fn add_draft_edit_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: AddItemAction,
) -> Result<ChangeActionView> {
    let change = open_edit(tx, ctx, id).await?;
    add_order_edit_item(tx, ctx, change.id, input).await
}

pub async fn remove_draft_edit_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    action_id: Uuid,
) -> Result<()> {
    let change = open_edit(tx, ctx, id).await?;
    remove_order_edit_item(tx, ctx, change.id, action_id).await
}

pub async fn add_draft_edit_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    let change = open_edit(tx, ctx, id).await?;
    add_order_edit_shipping(tx, ctx, change.id, input).await
}

pub async fn remove_draft_edit_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    action_id: Uuid,
) -> Result<()> {
    remove_draft_edit_item(tx, ctx, id, action_id).await
}

pub async fn confirm_draft_edit(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderId) -> Result<OrderView> {
    let change = open_edit(tx, ctx, id).await?;
    confirm_order_edit(tx, ctx, change.id).await
}

pub async fn decline_draft_edit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: DeclineChange,
) -> Result<ChangeView> {
    let change = open_edit(tx, ctx, id).await?;
    decline_order_edit(tx, ctx, change.id, input).await
}

// ---------------------------------------------------------------------------
// Returns
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReturnLineIn {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub return_reason_id: Option<Uuid>,
    pub note: Option<String>,
}

impl ReturnLineIn {
    fn line(self) -> order::ReturnLine {
        order::ReturnLine {
            order_line_item_id: self.order_line_item_id,
            quantity: self.quantity,
            return_reason_id: self.return_reason_id,
            note: self.note,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestReturn {
    pub order_id: OrderId,
    pub location_id: Option<StockLocationId>,
    pub lines: Vec<ReturnLineIn>,
}

pub async fn request_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RequestReturn,
) -> Result<ReturnView> {
    let lines = input.lines.into_iter().map(ReturnLineIn::line).collect();
    Ok(
        order::request_return(tx, ctx, input.order_id, input.location_id, lines)
            .await?
            .into(),
    )
}

/// Every return in the shop, newest last.
pub async fn list_returns(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ReturnView>> {
    let _: Permit = ctx.permit(
        Action::View,
        order_resource(OrderId::from_uuid(Uuid::nil()), None),
    )?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, order::Return>(&format!(
        "select {RETURN_COLUMNS} from order_return
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4"
    ))
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(page.map(ReturnView::from))
}

pub async fn get_return(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReturnId) -> Result<ReturnView> {
    let row = read_return(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::View, order_resource(row.order_id, None))?;
    Ok(row.into())
}

pub async fn return_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
) -> Result<Vec<ReturnItemView>> {
    let rows = order::return_items(tx, ctx, id).await?;
    Ok(rows.into_iter().map(ReturnItemView::from).collect())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceivedLineIn {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    #[serde(default)]
    pub damaged: i32,
}

impl ReceivedLineIn {
    fn line(self) -> order::ReceivedLine {
        order::ReceivedLine {
            order_line_item_id: self.order_line_item_id,
            quantity: self.quantity,
            damaged: self.damaged,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReceiveReturn {
    pub lines: Vec<ReceivedLineIn>,
}

pub async fn receive_return(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    input: ReceiveReturn,
) -> Result<ReturnView> {
    let lines = input.lines.into_iter().map(ReceivedLineIn::line).collect();
    Ok(order::receive_return(tx, ctx, id, lines).await?.into())
}

pub async fn dismiss_return_items(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    input: ReceiveReturn,
) -> Result<ReturnView> {
    let lines = input.lines.into_iter().map(ReceivedLineIn::line).collect();
    Ok(order::dismiss_return(tx, ctx, id, lines).await?.into())
}

/// Declines the change still open on a return and closes it.
pub async fn cancel_return(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ReturnId) -> Result<ReturnView> {
    let order_return = read_return(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::Write, order_resource(order_return.order_id, None))?;

    if order_return.received_at.is_some() {
        return Err(Error::conflict("that return has already arrived"));
    }
    if order_return.canceled_at.is_some() {
        return Ok(order_return.into());
    }

    if let Ok(change) = open_change(tx, ctx, Parent::Return, id.as_uuid()).await {
        order::decline_change(tx, ctx, change.id, Some("the return was canceled".into())).await?;
    }

    sqlx::query(
        "update order_return set status = 'canceled', canceled_at = $3
         where scope = $1 and id = $2 and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    Ok(read_return(tx, ctx, id).await?.into())
}

/// Hangs one more action on the change a return already has open. There is no
/// second path: `order_change` is how a quantity moves.
async fn add_parent_action(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    parent: Parent,
    id: Uuid,
    new: order::NewAction,
) -> Result<ChangeActionView> {
    let change = open_change(tx, ctx, parent, id).await?;
    Ok(order::add_action(tx, ctx, change.id, new).await?.into())
}

async fn drop_parent_action(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    parent: Parent,
    id: Uuid,
    action_id: Uuid,
) -> Result<()> {
    let change = open_change(tx, ctx, parent, id).await?;
    let _: Permit = ctx.permit(Action::Write, order_resource(change.order_id, None))?;
    drop_action(tx, ctx, change.id, action_id).await
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineQuantity {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
}

pub async fn add_return_request_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ReturnItem,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Return, id.as_uuid(), new).await
}

pub async fn remove_return_request_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Return, id.as_uuid(), action_id).await
}

pub async fn add_return_receive_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ReceiveReturnItem,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Return, id.as_uuid(), new).await
}

pub async fn remove_return_receive_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Return, id.as_uuid(), action_id).await
}

pub async fn add_return_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    add_parent_action(tx, ctx, Parent::Return, id.as_uuid(), input.new_action()?).await
}

pub async fn remove_return_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Return, id.as_uuid(), action_id).await
}

/// Settles the return's open change: the quantities land at the next version.
pub async fn confirm_return_request(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ReturnId,
) -> Result<ReturnView> {
    let change = open_change(tx, ctx, Parent::Return, id.as_uuid()).await?;
    order::confirm_change(tx, ctx, change.id).await?;
    Ok(read_return(tx, ctx, id).await?.into())
}

// ---------------------------------------------------------------------------
// Exchanges
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExchangeLineIn {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestExchange {
    pub order_id: OrderId,
    pub returning: Vec<ReturnLineIn>,
    pub outbound: Vec<ExchangeLineIn>,
    pub location_id: Option<StockLocationId>,
    #[serde(default)]
    pub allow_backorder: bool,
    pub difference_due: Option<MoneyIn>,
}

pub async fn request_exchange(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RequestExchange,
) -> Result<ExchangeView> {
    let difference_due = match &input.difference_due {
        Some(money) => Some(money.money()?),
        None => None,
    };

    let request = order::ExchangeRequest {
        returning: input
            .returning
            .into_iter()
            .map(ReturnLineIn::line)
            .collect(),
        outbound: input
            .outbound
            .into_iter()
            .map(|line| order::ExchangeLine {
                order_line_item_id: line.order_line_item_id,
                quantity: line.quantity,
                note: line.note,
            })
            .collect(),
        location_id: input.location_id,
        allow_backorder: input.allow_backorder,
        difference_due,
    };

    Ok(order::request_exchange(tx, ctx, input.order_id, request)
        .await?
        .into())
}

pub async fn list_exchanges(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ExchangeView>> {
    let _: Permit = ctx.permit(
        Action::View,
        order_resource(OrderId::from_uuid(Uuid::nil()), None),
    )?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, order::Exchange>(&format!(
        "select {EXCHANGE_COLUMNS} from order_exchange
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4"
    ))
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(page.map(ExchangeView::from))
}

pub async fn get_exchange(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ExchangeId) -> Result<ExchangeView> {
    let row = read_exchange(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::View, order_resource(row.order_id, None))?;
    Ok(row.into())
}

pub async fn exchange_actions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
) -> Result<ChangeDetailView> {
    let change = open_change(tx, ctx, Parent::Exchange, id.as_uuid()).await?;
    get_order_change(tx, ctx, change.id).await
}

pub async fn cancel_exchange(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
) -> Result<ExchangeView> {
    let exchange = read_exchange(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::Write, order_resource(exchange.order_id, None))?;

    if exchange.canceled_at.is_some() {
        return Ok(exchange.into());
    }

    if let Ok(change) = open_change(tx, ctx, Parent::Exchange, id.as_uuid()).await {
        order::decline_change(tx, ctx, change.id, Some("the exchange was canceled".into())).await?;
    }

    sqlx::query(
        "update order_exchange set canceled_at = $3
         where scope = $1 and id = $2 and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    Ok(read_exchange(tx, ctx, id).await?.into())
}

pub async fn add_exchange_inbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ReturnItem,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), new).await
}

pub async fn remove_exchange_inbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), action_id).await
}

pub async fn add_exchange_inbound_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    add_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), input.new_action()?).await
}

pub async fn add_exchange_outbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ItemAdd,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), new).await
}

pub async fn remove_exchange_outbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), action_id).await
}

pub async fn add_exchange_outbound_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    add_parent_action(tx, ctx, Parent::Exchange, id.as_uuid(), input.new_action()?).await
}

pub async fn confirm_exchange_request(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ExchangeId,
) -> Result<ExchangeView> {
    let change = open_change(tx, ctx, Parent::Exchange, id.as_uuid()).await?;
    order::confirm_change(tx, ctx, change.id).await?;
    Ok(read_exchange(tx, ctx, id).await?.into())
}

// ---------------------------------------------------------------------------
// Claims
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    Refund,
    Replace,
}

impl ClaimKind {
    fn claim_type(self) -> order::ClaimType {
        match self {
            ClaimKind::Refund => order::ClaimType::Refund,
            ClaimKind::Replace => order::ClaimType::Replace,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimLineIn {
    pub order_line_item_id: LineItemId,
    pub quantity: i32,
    pub reason: Option<String>,
    pub note: Option<String>,
    /// Photos of the damage, as URLs into wherever the host keeps files —
    /// tezgah stores none of its own.
    #[serde(default)]
    pub images: Vec<String>,
}

impl ClaimLineIn {
    fn line(self) -> order::ClaimLine {
        order::ClaimLine {
            order_line_item_id: self.order_line_item_id,
            quantity: self.quantity,
            reason: self.reason,
            note: self.note,
            images: self.images,
        }
    }
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestClaim {
    pub order_id: OrderId,
    pub claim_type: ClaimKind,
    pub faulty: Vec<ClaimLineIn>,
    #[serde(default)]
    pub replacements: Vec<ClaimLineIn>,
    #[serde(default)]
    pub collect: bool,
    pub location_id: Option<StockLocationId>,
    pub refund_amount: Option<MoneyIn>,
}

pub async fn request_claim(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RequestClaim,
) -> Result<ClaimView> {
    let refund_amount = match &input.refund_amount {
        Some(money) => Some(money.money()?),
        None => None,
    };

    let request = order::ClaimRequest {
        claim_type: input.claim_type.claim_type(),
        faulty: input.faulty.into_iter().map(ClaimLineIn::line).collect(),
        replacements: input
            .replacements
            .into_iter()
            .map(ClaimLineIn::line)
            .collect(),
        collect: input.collect,
        location_id: input.location_id,
        refund_amount,
    };

    Ok(order::request_claim(tx, ctx, input.order_id, request)
        .await?
        .into())
}

pub async fn list_claims(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ClaimView>> {
    let _: Permit = ctx.permit(
        Action::View,
        order_resource(OrderId::from_uuid(Uuid::nil()), None),
    )?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, order::Claim>(&format!(
        "select {CLAIM_COLUMNS} from order_claim
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4"
    ))
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(page.map(ClaimView::from))
}

pub async fn get_claim(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ClaimId) -> Result<ClaimView> {
    let row = read_claim(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::View, order_resource(row.order_id, None))?;
    Ok(row.into())
}

/// What was actually claimed: one row per faulty or replacement line, with
/// whatever photos of the damage came in with it.
pub async fn claim_lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
) -> Result<Vec<ClaimItemView>> {
    let rows = order::claim_items(tx, ctx, id).await?;
    Ok(rows.into_iter().map(ClaimItemView::from).collect())
}

pub async fn claim_actions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
) -> Result<ChangeDetailView> {
    let change = open_change(tx, ctx, Parent::Claim, id.as_uuid()).await?;
    get_order_change(tx, ctx, change.id).await
}

pub async fn cancel_claim(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: ClaimId) -> Result<ClaimView> {
    let claim = read_claim(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::Write, order_resource(claim.order_id, None))?;

    if claim.canceled_at.is_some() {
        return Ok(claim.into());
    }

    if let Ok(change) = open_change(tx, ctx, Parent::Claim, id.as_uuid()).await {
        order::decline_change(tx, ctx, change.id, Some("the claim was canceled".into())).await?;
    }

    sqlx::query(
        "update order_claim set canceled_at = $3
         where scope = $1 and id = $2 and canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    Ok(read_claim(tx, ctx, id).await?.into())
}

/// What went wrong, written off where it stands rather than expected back.
pub async fn add_claim_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::WriteOffItem,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), new).await
}

pub async fn remove_claim_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), action_id).await
}

/// The faulty goods are wanted back after all.
pub async fn add_claim_inbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ReturnItem,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), new).await
}

pub async fn remove_claim_inbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), action_id).await
}

pub async fn add_claim_inbound_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    add_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), input.new_action()?).await
}

pub async fn add_claim_outbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    input: LineQuantity,
) -> Result<ChangeActionView> {
    let new = order::NewAction::on(
        order::ChangeAction::ItemAdd,
        input.order_line_item_id,
        input.quantity,
    );
    add_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), new).await
}

pub async fn remove_claim_outbound_item(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    action_id: Uuid,
) -> Result<()> {
    drop_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), action_id).await
}

pub async fn add_claim_outbound_shipping(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
    input: AddShippingAction,
) -> Result<ChangeActionView> {
    add_parent_action(tx, ctx, Parent::Claim, id.as_uuid(), input.new_action()?).await
}

pub async fn confirm_claim_request(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ClaimId,
) -> Result<ClaimView> {
    let change = open_change(tx, ctx, Parent::Claim, id.as_uuid()).await?;
    order::confirm_change(tx, ctx, change.id).await?;
    Ok(read_claim(tx, ctx, id).await?.into())
}

// ---------------------------------------------------------------------------
// Payments — the two that move money
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListPayments {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub payment_collection_id: Option<PaymentCollectionId>,
}

pub async fn list_payments(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListPayments,
) -> Result<Page<PaymentView>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Payment {
            id: Uuid::nil(),
            order: Uuid::nil(),
            customer: None,
        },
    )?;

    let paging = match &query.after {
        Some(text) => Paging::after(
            Cursor::decode(text)?,
            query.limit.unwrap_or(crate::page::DEFAULT_LIMIT),
        ),
        None => Paging::first(query.limit.unwrap_or(crate::page::DEFAULT_LIMIT)),
    };

    let rows = sqlx::query_as::<_, payment::Payment>(&format!(
        "select {PAYMENT_COLUMNS} from payment
         where scope = $1
           and ($2::uuid is null or payment_collection_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(
        query
            .payment_collection_id
            .map(PaymentCollectionId::as_uuid),
    )
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(page.map(PaymentView::from))
}

pub async fn get_payment(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<PaymentView> {
    Ok(payment::payment(tx, ctx, id).await?.into())
}

/// What a provider sends, once the host has checked the signature.
///
/// The signature is not in here on purpose: it is over bytes this type has
/// already been parsed out of, and checking it against a re-serialised body
/// is how a valid signature stops matching. The host verifies the request it
/// received and then calls this.
///
/// `payload` is the provider's own body, kept verbatim — the audit trail
/// wants what arrived rather than what tezgah understood of it.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderCallback {
    /// The provider's own id for this delivery. The same one arrives again
    /// when they redeliver, which is what makes the write idempotent.
    pub event_id: String,
    /// The provider's name for it, kept for the audit trail.
    pub event_type: String,
    pub kind: payment::WebhookKind,
    pub session_id: Option<PaymentSessionId>,
    pub amount: Option<MoneyIn>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CallbackView {
    /// `false` when this delivery has been seen before. The provider is
    /// acknowledged either way; a redelivery just changes nothing.
    pub recorded: bool,
    pub id: Option<PaymentWebhookEventId>,
}

/// Writes down what a provider said before anything acts on it.
///
/// A redelivery lands once: the insert is `on conflict do nothing` against
/// `(scope, provider, event_id)`, so the second arrival writes no row, changes
/// nothing, and is acknowledged. That is the whole reason a provider's
/// callback goes through a table rather than straight into a capture.
///
/// Recording is deliberately all this does. Acting on the event — capturing,
/// moving an order's state — is a second step against a row that is now
/// durable, so a crash between the two resumes rather than loses.
pub async fn receive_callback(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    provider_code: &str,
    body: ProviderCallback,
) -> Result<CallbackView> {
    let amount = match body.amount {
        Some(amount) => Some(amount.money()?),
        None => None,
    };

    let event = payment::WebhookEvent {
        event_id: body.event_id,
        kind: body.kind,
        event_type: body.event_type,
        session_id: body.session_id,
        amount,
        payload: body.payload,
    };

    match payment::record_webhook(tx, ctx, provider_code, &event).await? {
        payment::WebhookOutcome::Fresh { id } => Ok(CallbackView {
            recorded: true,
            id: Some(id),
        }),
        payment::WebhookOutcome::AlreadySeen => Ok(CallbackView {
            recorded: false,
            id: None,
        }),
    }
}

#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PendingCallbackView {
    pub id: PaymentWebhookEventId,
    pub payment_provider_id: PaymentProviderId,
    pub event_id: String,
    pub event_type: String,
    pub payload: Value,
    pub attempts: i32,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

impl From<payment::PendingWebhook> for PendingCallbackView {
    fn from(row: payment::PendingWebhook) -> Self {
        PendingCallbackView {
            id: row.id,
            payment_provider_id: row.payment_provider_id,
            event_id: row.event_id,
            event_type: row.event_type,
            payload: row.payload,
            attempts: row.attempts,
            received_at: row.received_at,
        }
    }
}

/// Its own struct rather than `api::PagingQuery`, which carries `JsonSchema`
/// and deliberately no `Deserialize` — it exists to be described once in the
/// document, not to be read from a query string. This is how the other lists
/// in this module take theirs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCallbacks {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

/// What arrived and has not been acted on, oldest first.
pub async fn pending_callbacks(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListCallbacks,
) -> Result<Page<PendingCallbackView>> {
    let limit = query.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    let paging = match query.after.as_deref() {
        Some(text) => Paging::after(Cursor::decode(text)?, limit),
        None => Paging::first(limit),
    };

    let page = payment::unprocessed(tx, ctx, paging).await?;
    Ok(page.map(PendingCallbackView::from))
}

/// Says one has been acted on, so it stops coming back.
pub async fn callback_processed(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentWebhookEventId,
) -> Result<()> {
    payment::mark_processed(tx, ctx, id).await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturePayment {
    pub amount: MoneyIn,
    pub metadata: Option<Value>,
}

/// Takes money that was only being held.
///
/// [`Action::Settle`], asked by `settlement::capture` before it reads
/// anything. There is no compensation for this one: the only way back is a
/// refund, which is another provider call and another row.
pub async fn capture_payment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    input: CapturePayment,
) -> Result<CaptureView> {
    let amount = input.amount.money()?;
    let capture = settlement::capture(tx, ctx, id, amount, input.metadata).await?;
    Ok(capture.into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefundPayment {
    pub amount: MoneyIn,
    pub reason_id: Option<RefundReasonId>,
    pub note: Option<String>,
}

/// Gives money back. Never more than was captured: `settlement::refund` checks
/// it with the payment row locked, because two refunds always turn up at once.
/// Leaves a captured-then-printed gift card alone; see `settlement`'s module
/// docs.
pub async fn refund_payment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    input: RefundPayment,
) -> Result<RefundView> {
    let amount = input.amount.money()?;
    let refund = settlement::refund(tx, ctx, id, amount, input.reason_id, input.note).await?;
    Ok(refund.into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefundToCredit {
    pub amount: MoneyIn,
    pub reason: Option<String>,
}

/// Gives an order's money to the customer's balance instead of back to the
/// card. `credit::refund_to_credit` asks `Action::Settle` on the credit it is
/// about to create, not on the order — moving money onto a balance is a
/// different power from moving it onto the order itself.
pub async fn refund_order_to_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderId,
    input: RefundToCredit,
) -> Result<StoreCreditView> {
    let amount = input.amount.money()?;
    Ok(StoreCreditView::from(
        credit::refund_to_credit(tx, ctx, id, amount, input.reason).await?,
    ))
}

pub async fn payment_providers(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<ProviderView>> {
    let rows = payment::providers(tx, ctx).await?;
    Ok(rows
        .into_iter()
        .map(|row| ProviderView {
            id: row.id.as_uuid(),
            code: row.code,
            is_enabled: row.is_enabled,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterPaymentProvider {
    pub code: String,
}

pub async fn register_payment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RegisterPaymentProvider,
) -> Result<ProviderView> {
    let row = payment::register_provider(tx, ctx, &input.code).await?;
    Ok(ProviderView {
        id: row.id.as_uuid(),
        code: row.code,
        is_enabled: row.is_enabled,
    })
}

/// Stops offering a provider whose card network started declining
/// everything. [`payment::create_session`] is what actually refuses a new
/// session against a disabled provider; a payment already collected keeps
/// its provider row and is untouched.
pub async fn disable_payment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentProviderId,
) -> Result<ProviderView> {
    let row = payment::set_provider_enabled(tx, ctx, id, false).await?;
    Ok(ProviderView {
        id: row.id.as_uuid(),
        code: row.code,
        is_enabled: row.is_enabled,
    })
}

pub async fn enable_payment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentProviderId,
) -> Result<ProviderView> {
    let row = payment::set_provider_enabled(tx, ctx, id, true).await?;
    Ok(ProviderView {
        id: row.id.as_uuid(),
        code: row.code,
        is_enabled: row.is_enabled,
    })
}

// ---------------------------------------------------------------------------
// Payment collections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCollection {
    pub amount: MoneyIn,
    pub metadata: Option<Value>,
}

pub async fn create_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateCollection,
) -> Result<CollectionView> {
    let new = payment::NewCollection {
        amount: input.amount.money()?,
        cart_id: None,
        metadata: input.metadata,
    };
    Ok(payment::create_collection(tx, ctx, new).await?.into())
}

pub async fn get_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
) -> Result<CollectionView> {
    Ok(payment::collection(tx, ctx, id).await?.into())
}

pub async fn payment_sessions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
    query: Listing,
) -> Result<Page<SessionView>> {
    let page = payment::sessions(tx, ctx, id, query.paging()?).await?;
    Ok(page.map(SessionView::from))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSession {
    pub provider_code: String,
    pub amount: MoneyIn,
    pub context: Option<Value>,
}

pub async fn create_payment_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
    input: CreateSession,
) -> Result<SessionView> {
    let new = payment::NewSession {
        collection_id: id,
        provider_code: input.provider_code,
        amount: input.amount.money()?,
        context: input.context,
        installment_count: None,
    };
    Ok(payment::create_session(tx, ctx, new).await?.into())
}

// ---------------------------------------------------------------------------
// Reasons
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NewReason {
    pub code: String,
    pub label: String,
    pub description: Option<String>,
}

pub async fn list_refund_reasons(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ReasonView>> {
    let _: Permit = ctx.permit(Action::View, config())?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, ReasonView>(
        "select id, code, label, description, created_at from refund_reason
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id)
    }))
}

pub async fn create_refund_reason(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: NewReason,
) -> Result<ReasonView> {
    let _: Permit = ctx.permit(Action::Write, config())?;

    if input.code.trim().is_empty() {
        return Err(Error::invalid("a refund reason needs a code"));
    }

    Ok(sqlx::query_as::<_, ReasonView>(
        "insert into refund_reason (id, scope, code, label, description)
         values ($1, $2, $3, $4, $5)
         returning id, code, label, description, created_at",
    )
    .bind(RefundReasonId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(input.code.trim())
    .bind(input.label)
    .bind(input.description)
    .fetch_one(&mut **tx)
    .await?)
}

/// The same shape, and a different column: a return reason's code is `value`.
pub async fn list_return_reasons(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ReasonView>> {
    let _: Permit = ctx.permit(Action::View, config())?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, ReasonView>(
        "select id, value as code, label, description, created_at from return_reason
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id)
    }))
}

pub async fn create_return_reason(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: NewReason,
) -> Result<ReasonView> {
    let _: Permit = ctx.permit(Action::Write, config())?;

    if input.code.trim().is_empty() {
        return Err(Error::invalid("a return reason needs a value"));
    }

    Ok(sqlx::query_as::<_, ReasonView>(
        "insert into return_reason (id, scope, value, label, description)
         values ($1, $2, $3, $4, $5)
         returning id, value as code, label, description, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(input.code.trim())
    .bind(input.label)
    .bind(input.description)
    .fetch_one(&mut **tx)
    .await?)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ReturnReasonTranslationView {
    pub return_reason_id: Uuid,
    pub locale: String,
    pub label: String,
    pub description: Option<String>,
}

impl From<order::ReturnReasonTranslation> for ReturnReasonTranslationView {
    fn from(row: order::ReturnReasonTranslation) -> Self {
        ReturnReasonTranslationView {
            return_reason_id: row.return_reason_id,
            locale: row.locale,
            label: row.label,
            description: row.description,
        }
    }
}

pub async fn list_return_reason_translations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_reason_id: Uuid,
) -> Result<Vec<ReturnReasonTranslationView>> {
    let rows = order::return_reason_translations(tx, ctx, return_reason_id).await?;
    Ok(rows
        .into_iter()
        .map(ReturnReasonTranslationView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PutReturnReasonTranslation {
    pub locale: String,
    pub label: String,
    pub description: Option<String>,
}

pub async fn put_return_reason_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_reason_id: Uuid,
    body: PutReturnReasonTranslation,
) -> Result<ReturnReasonTranslationView> {
    let translation = order::ReturnReasonTranslation {
        return_reason_id,
        locale: body.locale,
        label: body.label,
        description: body.description,
    };
    Ok(ReturnReasonTranslationView::from(
        order::put_return_reason_translation(tx, ctx, return_reason_id, translation).await?,
    ))
}

pub async fn remove_return_reason_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_reason_id: Uuid,
    locale: &str,
) -> Result<()> {
    order::remove_return_reason_translation(tx, ctx, return_reason_id, locale).await
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LocalisedReturnReasonView {
    pub return_reason_id: Uuid,
    pub locale: Option<String>,
    pub label: String,
    pub description: Option<String>,
    pub is_fallback: bool,
}

impl From<order::LocalisedReturnReason> for LocalisedReturnReasonView {
    fn from(row: order::LocalisedReturnReason) -> Self {
        LocalisedReturnReasonView {
            return_reason_id: row.return_reason_id,
            locale: row.locale,
            label: row.label,
            description: row.description,
            is_fallback: row.is_fallback,
        }
    }
}

pub async fn localised_return_reason(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_reason_id: Uuid,
    locale: &str,
) -> Result<LocalisedReturnReasonView> {
    Ok(LocalisedReturnReasonView::from(
        order::localised_return_reason(tx, ctx, return_reason_id, locale).await?,
    ))
}

// ---------------------------------------------------------------------------
// Fulfilment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewFulfillmentItemIn {
    pub order_item_id: OrderItemId,
    pub inventory_item_id: Option<InventoryItemId>,
    pub title: String,
    pub sku: Option<String>,
    pub barcode: Option<String>,
    pub quantity: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFulfillment {
    pub location_id: StockLocationId,
    pub shipping_option_id: Option<ShippingOptionId>,
    pub provider_id: Option<Uuid>,
    #[serde(default = "yes")]
    pub requires_shipping: bool,
    pub data: Option<Value>,
    pub items: Vec<NewFulfillmentItemIn>,
}

fn yes() -> bool {
    true
}

pub async fn create_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    input: CreateFulfillment,
) -> Result<FulfillmentView> {
    let new = fulfilment::NewFulfillment {
        location_id: input.location_id,
        shipping_option_id: input.shipping_option_id,
        provider_id: input.provider_id,
        requires_shipping: input.requires_shipping,
        created_by: None,
        address: None,
        data: input.data,
        items: input
            .items
            .into_iter()
            .map(|item| fulfilment::NewFulfillmentItem {
                order_item_id: item.order_item_id,
                inventory_item_id: item.inventory_item_id,
                title: item.title,
                sku: item.sku,
                barcode: item.barcode,
                quantity: item.quantity,
            })
            .collect(),
    };

    Ok(fulfilment::create_fulfillment(tx, ctx, order_id, new)
        .await?
        .into())
}

/// Every parcel on one order. Reached through the fulfilment items, because a
/// `fulfillment` row does not carry the order it belongs to; the column called
/// `line_item_id` holds an `order_item` id, not an `order_line_item` one.
pub async fn order_fulfillments(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    query: Listing,
) -> Result<Page<FulfillmentView>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Fulfillment {
            id: Uuid::nil(),
            order: order_id.as_uuid(),
        },
    )?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, fulfilment::Fulfillment>(
        "select f.id, f.location_id, f.shipping_option_id, f.provider_id, f.requires_shipping,
                f.packed_at, f.shipped_at, f.delivered_at, f.canceled_at, f.created_at
         from fulfillment f
         where f.scope = $1
           and exists (
               select 1 from fulfillment_item i
               join order_item oi on oi.scope = i.scope and oi.id = i.line_item_id
               where i.scope = f.scope and i.fulfillment_id = f.id and oi.order_id = $2
           )
           and ($3::timestamptz is null or (f.created_at, f.id) > ($3, $4))
         order by f.created_at, f.id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(page.map(FulfillmentView::from))
}

pub async fn get_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    id: FulfillmentId,
) -> Result<FulfillmentDetailView> {
    let row = fulfilment::fulfillment(tx, ctx, order_id, id).await?;
    let items = fulfilment::fulfillment_items(tx, ctx, order_id, id).await?;
    let labels = fulfilment::labels(tx, ctx, order_id, id).await?;

    Ok(FulfillmentDetailView {
        fulfillment: row.into(),
        items: items
            .into_iter()
            .map(|item| FulfillmentItemView {
                id: item.id,
                title: item.title,
                sku: item.sku,
                quantity: item.quantity,
            })
            .collect(),
        labels: labels
            .into_iter()
            .map(|label| LabelView {
                id: label.id,
                tracking_number: label.tracking_number,
                tracking_url: label.tracking_url,
                label_url: label.label_url,
            })
            .collect(),
    })
}

pub async fn pack_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    id: FulfillmentId,
) -> Result<FulfillmentView> {
    Ok(fulfilment::mark_packed(tx, ctx, order_id, id).await?.into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShipFulfillment {
    #[serde(default)]
    pub labels: Vec<NewLabelIn>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLabelIn {
    pub tracking_number: String,
    pub tracking_url: Option<String>,
    pub label_url: Option<String>,
}

/// The parcel left. The labels go on in the same transaction, so a shipment
/// that rolls back does not leave a tracking number pointing at nothing.
pub async fn ship_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    id: FulfillmentId,
    input: ShipFulfillment,
) -> Result<FulfillmentDetailView> {
    for label in input.labels {
        fulfilment::add_label(
            tx,
            ctx,
            order_id,
            id,
            fulfilment::NewLabel {
                tracking_number: label.tracking_number,
                tracking_url: label.tracking_url,
                label_url: label.label_url,
            },
        )
        .await?;
    }

    fulfilment::mark_shipped(tx, ctx, order_id, id, None).await?;
    get_fulfillment(tx, ctx, order_id, id).await
}

pub async fn deliver_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    id: FulfillmentId,
) -> Result<FulfillmentView> {
    Ok(fulfilment::mark_delivered(tx, ctx, order_id, id)
        .await?
        .into())
}

/// Cancelling a parcel puts its stock back, which is why `fulfilment` asks for
/// [`Action::Settle`] rather than [`Action::Write`].
pub async fn cancel_fulfillment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    id: FulfillmentId,
) -> Result<FulfillmentView> {
    Ok(fulfilment::cancel_fulfillment(tx, ctx, order_id, id)
        .await?
        .into())
}

// ---------------------------------------------------------------------------
// Fulfilment configuration
// ---------------------------------------------------------------------------

pub async fn list_fulfillment_sets(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<FulfillmentSetView>> {
    let page = fulfilment::sets(tx, ctx, query.paging()?).await?;
    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(FulfillmentSetView::from)
            .collect(),
        next: page.next,
        total: page.total,
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetKindIn {
    Shipping,
    Pickup,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFulfillmentSet {
    pub name: String,
    pub kind: SetKindIn,
}

pub async fn create_fulfillment_set(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateFulfillmentSet,
) -> Result<FulfillmentSetView> {
    let new = fulfilment::NewFulfillmentSet {
        name: input.name,
        kind: match input.kind {
            SetKindIn::Shipping => fulfilment::SetKind::Shipping,
            SetKindIn::Pickup => fulfilment::SetKind::Pickup,
        },
    };
    Ok(fulfilment::create_set(tx, ctx, new).await?.into())
}

pub async fn delete_fulfillment_set(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentSetId,
) -> Result<()> {
    fulfilment::delete_set(tx, ctx, id).await
}

pub async fn service_zones(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentSetId,
) -> Result<Vec<ServiceZoneView>> {
    let rows = fulfilment::service_zones(tx, ctx, id).await?;
    Ok(rows
        .into_iter()
        .map(|row| ServiceZoneView {
            id: row.id,
            name: row.name,
            fulfillment_set_id: row.fulfillment_set_id,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateServiceZone {
    pub name: String,
}

pub async fn create_service_zone(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: FulfillmentSetId,
    input: CreateServiceZone,
) -> Result<ServiceZoneView> {
    let zone = fulfilment::create_service_zone(
        tx,
        ctx,
        fulfilment::NewServiceZone {
            name: input.name,
            fulfillment_set_id: id,
        },
    )
    .await?;

    Ok(ServiceZoneView {
        id: zone.id,
        name: zone.name,
        fulfillment_set_id: zone.fulfillment_set_id,
    })
}

pub async fn fulfillment_providers(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<ProviderView>> {
    let rows = fulfilment::providers(tx, ctx).await?;
    Ok(rows
        .into_iter()
        .map(|row| ProviderView {
            id: row.id,
            code: row.name,
            is_enabled: row.is_enabled,
        })
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterProvider {
    pub name: String,
}

pub async fn register_fulfillment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: RegisterProvider,
) -> Result<ProviderView> {
    let row = fulfilment::register_provider(tx, ctx, &input.name).await?;
    Ok(ProviderView {
        id: row.id,
        code: row.name,
        is_enabled: row.is_enabled,
    })
}

/// Stops offering a carrier a shop has dropped. Its shipments keep their
/// provider row; checkout stops offering the shipping options that pointed
/// to it.
pub async fn disable_fulfillment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Uuid,
) -> Result<ProviderView> {
    let row = fulfilment::set_provider_enabled(tx, ctx, id, false).await?;
    Ok(ProviderView {
        id: row.id,
        code: row.name,
        is_enabled: row.is_enabled,
    })
}

pub async fn enable_fulfillment_provider(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Uuid,
) -> Result<ProviderView> {
    let row = fulfilment::set_provider_enabled(tx, ctx, id, true).await?;
    Ok(ProviderView {
        id: row.id,
        code: row.name,
        is_enabled: row.is_enabled,
    })
}

pub async fn list_shipping_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ShippingOptionView>> {
    let _: Permit = ctx.permit(Action::View, config())?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, fulfilment::ShippingOption>(
        "select id, name, price_type, service_zone_id, shipping_profile_id, provider_id,
                shipping_option_type_id, data, is_return, enabled_in_store, created_at
         from shipping_option
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(ShippingOptionView::from)
            .collect(),
        next: page.next,
        total: page.total,
    })
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceKindIn {
    Flat,
    Calculated,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShippingOption {
    pub name: String,
    pub price_type: PriceKindIn,
    pub service_zone_id: ServiceZoneId,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub provider_id: Option<Uuid>,
    pub shipping_option_type_id: Option<Uuid>,
    pub data: Option<Value>,
    #[serde(default)]
    pub is_return: bool,
    #[serde(default = "yes")]
    pub enabled_in_store: bool,
}

pub async fn create_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateShippingOption,
) -> Result<ShippingOptionView> {
    let new = fulfilment::NewShippingOption {
        name: input.name,
        price_type: match input.price_type {
            PriceKindIn::Flat => fulfilment::PriceKind::Flat,
            PriceKindIn::Calculated => fulfilment::PriceKind::Calculated,
        },
        service_zone_id: input.service_zone_id,
        shipping_profile_id: input.shipping_profile_id,
        provider_id: input.provider_id,
        shipping_option_type_id: input.shipping_option_type_id,
        data: input.data,
        is_return: input.is_return,
        enabled_in_store: input.enabled_in_store,
    };
    Ok(fulfilment::create_shipping_option(tx, ctx, new)
        .await?
        .into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShippingOptionRule {
    pub attribute: String,
    pub operator: String,
    pub value: Value,
}

pub async fn create_shipping_option_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    input: CreateShippingOptionRule,
) -> Result<ShippingOptionRuleView> {
    let new = fulfilment::NewShippingOptionRule {
        shipping_option_id: id,
        attribute: input.attribute,
        operator: fulfilment::Operator::parse(&input.operator)?,
        value: input.value,
    };
    let rule = fulfilment::create_shipping_option_rule(tx, ctx, new).await?;

    Ok(ShippingOptionRuleView {
        id: rule.id,
        shipping_option_id: rule.shipping_option_id,
        attribute: rule.attribute,
        operator: rule.operator,
        value: rule.value,
    })
}

/// Which options reach one order's address, priced or not.
pub async fn order_shipping_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    country_code: &str,
) -> Result<Vec<ShippingOptionView>> {
    let row = order::get(tx, ctx, order_id).await?;

    let address = fulfilment::DeliveryAddress {
        country_code: country_code.to_owned(),
        ..Default::default()
    };

    let options = fulfilment::options_for(
        tx,
        ctx,
        &address,
        row.sales_channel_id,
        &[],
        fulfilment::ShippingAudience::Admin,
    )
    .await?;
    Ok(options.into_iter().map(ShippingOptionView::from).collect())
}

/// Which options a return may ship back on: only the ones a shop marked for
/// returns, so an operator is never offered the order's own delivery option
/// to send a parcel the other way.
pub async fn return_shipping_options(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    country_code: &str,
) -> Result<Vec<ShippingOptionView>> {
    let row = order::get(tx, ctx, order_id).await?;

    let address = fulfilment::DeliveryAddress {
        country_code: country_code.to_owned(),
        ..Default::default()
    };

    let options = fulfilment::options_for(
        tx,
        ctx,
        &address,
        row.sales_channel_id,
        &[],
        fulfilment::ShippingAudience::Return,
    )
    .await?;
    Ok(options.into_iter().map(ShippingOptionView::from).collect())
}

pub async fn list_shipping_profiles(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ShippingProfileView>> {
    let _: Permit = ctx.permit(Action::View, config())?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, ShippingProfileView>(
        "select id, name, type, created_at from shipping_profile
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    }))
}

pub async fn get_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
) -> Result<ShippingOptionView> {
    Ok(fulfilment::shipping_option(tx, ctx, id).await?.into())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateShippingOption {
    pub name: Option<String>,
    pub price_type: Option<PriceKindIn>,
    pub shipping_profile_id: Option<ShippingProfileId>,
    pub provider_id: Option<Uuid>,
    pub shipping_option_type_id: Option<Uuid>,
    pub data: Option<Value>,
    pub is_return: Option<bool>,
    pub enabled_in_store: Option<bool>,
}

pub async fn update_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    input: UpdateShippingOption,
) -> Result<ShippingOptionView> {
    let patch = fulfilment::ShippingOptionPatch {
        name: input.name,
        price_type: input.price_type.map(|kind| match kind {
            PriceKindIn::Flat => fulfilment::PriceKind::Flat,
            PriceKindIn::Calculated => fulfilment::PriceKind::Calculated,
        }),
        shipping_profile_id: input.shipping_profile_id,
        provider_id: input.provider_id,
        shipping_option_type_id: input.shipping_option_type_id,
        data: input.data,
        is_return: input.is_return,
        enabled_in_store: input.enabled_in_store,
    };
    Ok(fulfilment::update_shipping_option(tx, ctx, id, patch)
        .await?
        .into())
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShippingOptionTranslationView {
    pub shipping_option_id: ShippingOptionId,
    pub locale: String,
    pub name: String,
}

impl From<fulfilment::ShippingOptionTranslation> for ShippingOptionTranslationView {
    fn from(row: fulfilment::ShippingOptionTranslation) -> Self {
        ShippingOptionTranslationView {
            shipping_option_id: row.shipping_option_id,
            locale: row.locale,
            name: row.name,
        }
    }
}

pub async fn list_shipping_option_translations(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
) -> Result<Vec<ShippingOptionTranslationView>> {
    let rows = fulfilment::shipping_option_translations(tx, ctx, id).await?;
    Ok(rows
        .into_iter()
        .map(ShippingOptionTranslationView::from)
        .collect())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PutShippingOptionTranslation {
    pub locale: String,
    pub name: String,
}

pub async fn put_shipping_option_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    body: PutShippingOptionTranslation,
) -> Result<ShippingOptionTranslationView> {
    let translation = fulfilment::ShippingOptionTranslation {
        shipping_option_id: id,
        locale: body.locale,
        name: body.name,
    };
    Ok(ShippingOptionTranslationView::from(
        fulfilment::put_shipping_option_translation(tx, ctx, id, translation).await?,
    ))
}

pub async fn remove_shipping_option_translation(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    locale: &str,
) -> Result<()> {
    fulfilment::remove_shipping_option_translation(tx, ctx, id, locale).await
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct LocalisedShippingOptionView {
    pub shipping_option_id: ShippingOptionId,
    pub locale: Option<String>,
    pub name: String,
    pub is_fallback: bool,
}

impl From<fulfilment::LocalisedShippingOption> for LocalisedShippingOptionView {
    fn from(row: fulfilment::LocalisedShippingOption) -> Self {
        LocalisedShippingOptionView {
            shipping_option_id: row.shipping_option_id,
            locale: row.locale,
            name: row.name,
            is_fallback: row.is_fallback,
        }
    }
}

pub async fn localised_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingOptionId,
    locale: &str,
) -> Result<LocalisedShippingOptionView> {
    Ok(LocalisedShippingOptionView::from(
        fulfilment::localised_shipping_option(tx, ctx, id, locale).await?,
    ))
}

impl From<fulfilment::ShippingProfile> for ShippingProfileView {
    fn from(row: fulfilment::ShippingProfile) -> Self {
        ShippingProfileView {
            id: row.id,
            name: row.name,
            kind: row.kind,
            created_at: row.created_at,
        }
    }
}

pub async fn get_shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingProfileId,
) -> Result<ShippingProfileView> {
    Ok(ShippingProfileView::from(
        fulfilment::shipping_profile(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateShippingProfile {
    pub name: Option<String>,
    pub kind: Option<String>,
}

pub async fn update_shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: ShippingProfileId,
    input: UpdateShippingProfile,
) -> Result<ShippingProfileView> {
    let patch = fulfilment::ShippingProfilePatch {
        name: input.name,
        kind: input.kind,
    };
    Ok(ShippingProfileView::from(
        fulfilment::update_shipping_profile(tx, ctx, id, patch).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShippingProfile {
    pub name: String,
    pub kind: String,
}

pub async fn create_shipping_profile(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateShippingProfile,
) -> Result<ShippingProfileView> {
    let id = fulfilment::create_shipping_profile(tx, ctx, &input.name, &input.kind).await?;

    sqlx::query_as::<_, ShippingProfileView>(
        "select id, name, type, created_at from shipping_profile where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("shipping profile"))
}

pub async fn list_shipping_option_types(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: Listing,
) -> Result<Page<ShippingOptionTypeView>> {
    let _: Permit = ctx.permit(Action::View, config())?;
    let paging = query.paging()?;

    let rows = sqlx::query_as::<_, ShippingOptionTypeView>(
        "select id, label, code, description, created_at from shipping_option_type
         where scope = $1 and ($2::timestamptz is null or (created_at, id) > ($2, $3))
         order by created_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id)
    }))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateShippingOptionType {
    pub label: String,
    pub code: String,
    pub description: Option<String>,
}

pub async fn create_shipping_option_type(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    input: CreateShippingOptionType,
) -> Result<ShippingOptionTypeView> {
    let _: Permit = ctx.permit(Action::Write, config())?;

    if input.code.trim().is_empty() {
        return Err(Error::invalid("a shipping option type needs a code"));
    }

    Ok(sqlx::query_as::<_, ShippingOptionTypeView>(
        "insert into shipping_option_type (id, scope, label, code, description)
         values ($1, $2, $3, $4, $5)
         returning id, label, code, description, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(input.label)
    .bind(input.code.trim())
    .bind(input.description)
    .fetch_one(&mut **tx)
    .await?)
}

// ---------------------------------------------------------------------------
// The table
// ---------------------------------------------------------------------------

macro_rules! route {
    ($method:ident, $path:literal, $action:ident, $domain:literal, $summary:literal) => {
        Route {
            surface: Surface::Admin,
            method: Method::$method,
            path: $path,
            action: Action::$action,
            domain: $domain,
            query: None,
            summary: $summary,
        }
    };
}

/// The same, for a route that pages. Its own macro rather than a sixth
/// argument on `route!`, so that a list which forgets to say it pages reads
/// as a list that does not — which is what a reviewer can see.
macro_rules! paged {
    ($path:literal, $domain:literal, $summary:literal) => {
        Route {
            surface: Surface::Admin,
            method: Method::Get,
            path: $path,
            action: Action::View,
            domain: $domain,
            query: None,
            summary: $summary,
        }
    };
}

pub(super) static ROUTES: &[Route] = &[
    // Orders
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders",
        action: Action::View,
        domain: "order",
        query: Some(super::query_schema::<ListOrders>),
        summary: "List orders",
    },
    route!(Post, "/admin/orders", Write, "order", "Create an order"),
    route!(Get, "/admin/orders/{id}", View, "order", "Fetch one order"),
    route!(
        Post,
        "/admin/orders/{id}/complete",
        Write,
        "order",
        "Complete an order"
    ),
    route!(
        Post,
        "/admin/orders/{id}/cancel",
        Write,
        "order",
        "Cancel an order"
    ),
    route!(
        Post,
        "/admin/orders/{id}/archive",
        Write,
        "order",
        "Archive a completed order"
    ),
    route!(
        Patch,
        "/admin/orders/{id}/shipping-address",
        Write,
        "order",
        "Correct the address a parcel is going to"
    ),
    route!(
        Patch,
        "/admin/orders/{id}/billing-address",
        Write,
        "order",
        "Correct the order's billing address"
    ),
    route!(
        Patch,
        "/admin/orders/{id}/email",
        Write,
        "order",
        "Correct the order's e-mail address"
    ),
    route!(
        Get,
        "/admin/orders/{id}/line-items",
        View,
        "order",
        "List an order's lines"
    ),
    route!(
        Get,
        "/admin/orders/{id}/items",
        View,
        "order",
        "List an order's quantities at one version"
    ),
    route!(
        Get,
        "/admin/orders/{id}/shipping-methods",
        View,
        "order",
        "List an order's shipping methods"
    ),
    route!(
        Get,
        "/admin/orders/{id}/summary",
        View,
        "order",
        "Fetch the summary written at one version"
    ),
    route!(
        Get,
        "/admin/orders/{id}/totals",
        View,
        "order",
        "Work out an order's totals"
    ),
    route!(
        Get,
        "/admin/orders/{id}/ledger",
        View,
        "order",
        "What has been authorised, captured and refunded"
    ),
    route!(
        Get,
        "/admin/orders/{id}/transactions",
        View,
        "order",
        "List an order's money movements"
    ),
    route!(
        Post,
        "/admin/orders/{id}/transactions",
        Settle,
        "order",
        "Record money that moved outside tezgah"
    ),
    route!(
        Post,
        "/admin/orders/{id}/payment-collection",
        Write,
        "order",
        "Put a payment collection under a placed order"
    ),
    paged!(
        "/admin/orders/{id}/changes",
        "order",
        "List the changes made to an order"
    ),
    paged!(
        "/admin/orders/{id}/returns",
        "order",
        "List one order's returns"
    ),
    paged!(
        "/admin/orders/{id}/fulfillments",
        "fulfilment",
        "List one order's parcels"
    ),
    route!(
        Post,
        "/admin/orders/{id}/fulfillments",
        Write,
        "fulfilment",
        "Make up a parcel from part of an order"
    ),
    route!(
        Get,
        "/admin/orders/{id}/shipping-options",
        View,
        "fulfilment",
        "Which shipping options reach an order's address"
    ),
    route!(
        Get,
        "/admin/orders/{id}/returns/shipping-options",
        View,
        "fulfilment",
        "Which shipping options a return may ship back on"
    ),
    // Draft orders
    paged!("/admin/draft-orders", "order", "List draft orders"),
    route!(
        Post,
        "/admin/draft-orders",
        Write,
        "order",
        "Draw up a draft order"
    ),
    route!(
        Get,
        "/admin/draft-orders/{id}",
        View,
        "order",
        "Fetch one draft order"
    ),
    route!(
        Delete,
        "/admin/draft-orders/{id}",
        Write,
        "order",
        "Cancel a draft order"
    ),
    route!(
        Post,
        "/admin/draft-orders/{id}/convert-to-order",
        Write,
        "order",
        "Send a draft out to be paid"
    ),
    route!(
        Get,
        "/admin/draft-orders/{id}/edit",
        View,
        "order",
        "The edit open on a draft"
    ),
    route!(
        Post,
        "/admin/draft-orders/{id}/edit",
        Write,
        "order",
        "Open an edit on a draft"
    ),
    route!(
        Delete,
        "/admin/draft-orders/{id}/edit",
        Write,
        "order",
        "Decline the edit open on a draft"
    ),
    route!(
        Post,
        "/admin/draft-orders/{id}/edit/items",
        Write,
        "order",
        "Add, change or remove a line on a draft's edit"
    ),
    route!(
        Delete,
        "/admin/draft-orders/{id}/edit/items/{action_id}",
        Write,
        "order",
        "Take an action off a draft's edit"
    ),
    route!(
        Post,
        "/admin/draft-orders/{id}/edit/shipping-methods",
        Write,
        "order",
        "Add shipping to a draft's edit"
    ),
    route!(
        Delete,
        "/admin/draft-orders/{id}/edit/shipping-methods/{action_id}",
        Write,
        "order",
        "Take shipping off a draft's edit"
    ),
    route!(
        Post,
        "/admin/draft-orders/{id}/edit/confirm",
        Write,
        "order",
        "Carry a draft's edit into its next version"
    ),
    // Order edits
    paged!(
        "/admin/orders/{id}/order-edits",
        "order",
        "List the edits made to an order"
    ),
    route!(
        Post,
        "/admin/orders/{id}/order-edits",
        Write,
        "order",
        "Open an edit on an order"
    ),
    route!(
        Get,
        "/admin/order-edits/{id}",
        View,
        "order",
        "Fetch one edit and its actions"
    ),
    route!(
        Delete,
        "/admin/order-edits/{id}",
        Write,
        "order",
        "Decline an edit"
    ),
    route!(
        Post,
        "/admin/order-edits/{id}/items",
        Write,
        "order",
        "Add, change or remove a line on an edit"
    ),
    route!(
        Delete,
        "/admin/order-edits/{id}/items/{action_id}",
        Write,
        "order",
        "Take an action off an edit"
    ),
    route!(
        Post,
        "/admin/order-edits/{id}/shipping-method",
        Write,
        "order",
        "Add shipping to an edit"
    ),
    route!(
        Delete,
        "/admin/order-edits/{id}/shipping-method/{action_id}",
        Write,
        "order",
        "Take shipping off an edit"
    ),
    route!(
        Post,
        "/admin/order-edits/{id}/confirm",
        Write,
        "order",
        "Carry an edit into the order's next version"
    ),
    // Order changes
    route!(
        Get,
        "/admin/order-changes/{id}",
        View,
        "order",
        "Fetch one change and its actions"
    ),
    // Returns
    paged!("/admin/returns", "order", "List returns"),
    route!(Post, "/admin/returns", Write, "order", "Request a return"),
    route!(
        Get,
        "/admin/returns/{id}",
        View,
        "order",
        "Fetch one return"
    ),
    route!(
        Get,
        "/admin/returns/{id}/items",
        View,
        "order",
        "What is on a return"
    ),
    route!(
        Post,
        "/admin/returns/{id}/receive",
        Write,
        "order",
        "The parcel arrived; put what is sellable back"
    ),
    route!(
        Post,
        "/admin/returns/{id}/dismiss-items",
        Write,
        "order",
        "Received and not taken; no stock moves"
    ),
    route!(
        Post,
        "/admin/returns/{id}/cancel",
        Write,
        "order",
        "Cancel a return that has not arrived"
    ),
    route!(
        Post,
        "/admin/returns/{id}/request-items",
        Write,
        "order",
        "Add a line to the return's open change"
    ),
    route!(
        Delete,
        "/admin/returns/{id}/request-items/{action_id}",
        Write,
        "order",
        "Take a line off the return's open change"
    ),
    route!(
        Post,
        "/admin/returns/{id}/receive-items",
        Write,
        "order",
        "Record a line as received"
    ),
    route!(
        Delete,
        "/admin/returns/{id}/receive-items/{action_id}",
        Write,
        "order",
        "Take a received line back off"
    ),
    route!(
        Post,
        "/admin/returns/{id}/shipping-method",
        Write,
        "order",
        "Charge the return's carriage"
    ),
    route!(
        Delete,
        "/admin/returns/{id}/shipping-method/{action_id}",
        Write,
        "order",
        "Take the return's carriage off"
    ),
    route!(
        Post,
        "/admin/returns/{id}/request",
        Write,
        "order",
        "Settle the return's open change"
    ),
    // Exchanges
    paged!("/admin/exchanges", "order", "List exchanges"),
    route!(
        Post,
        "/admin/exchanges",
        Write,
        "order",
        "Request an exchange"
    ),
    route!(
        Get,
        "/admin/exchanges/{id}",
        View,
        "order",
        "Fetch one exchange"
    ),
    route!(
        Get,
        "/admin/exchanges/{id}/items",
        View,
        "order",
        "The exchange's open change and its actions"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/cancel",
        Write,
        "order",
        "Cancel an exchange"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/inbound/items",
        Write,
        "order",
        "Add something coming back"
    ),
    route!(
        Delete,
        "/admin/exchanges/{id}/inbound/items/{action_id}",
        Write,
        "order",
        "Take something coming back off"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/inbound/shipping-method",
        Write,
        "order",
        "Charge the carriage on what comes back"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/outbound/items",
        Write,
        "order",
        "Add something going out instead"
    ),
    route!(
        Delete,
        "/admin/exchanges/{id}/outbound/items/{action_id}",
        Write,
        "order",
        "Take something going out off"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/outbound/shipping-method",
        Write,
        "order",
        "Charge the carriage on what goes out"
    ),
    route!(
        Post,
        "/admin/exchanges/{id}/request",
        Write,
        "order",
        "Settle the exchange's open change"
    ),
    // Claims
    paged!("/admin/claims", "order", "List claims"),
    route!(Post, "/admin/claims", Write, "order", "Raise a claim"),
    route!(Get, "/admin/claims/{id}", View, "order", "Fetch one claim"),
    route!(
        Get,
        "/admin/claims/{id}/lines",
        View,
        "order",
        "What was claimed, faulty or replacement, with any photos"
    ),
    route!(
        Get,
        "/admin/claims/{id}/items",
        View,
        "order",
        "The claim's open change and its actions"
    ),
    route!(
        Post,
        "/admin/claims/{id}/cancel",
        Write,
        "order",
        "Cancel a claim"
    ),
    route!(
        Post,
        "/admin/claims/{id}/claim-items",
        Write,
        "order",
        "Write off what went wrong where it stands"
    ),
    route!(
        Delete,
        "/admin/claims/{id}/claim-items/{action_id}",
        Write,
        "order",
        "Take a written-off line back off"
    ),
    route!(
        Post,
        "/admin/claims/{id}/inbound/items",
        Write,
        "order",
        "Ask for the faulty goods back"
    ),
    route!(
        Delete,
        "/admin/claims/{id}/inbound/items/{action_id}",
        Write,
        "order",
        "Stop asking for a line back"
    ),
    route!(
        Post,
        "/admin/claims/{id}/inbound/shipping-method",
        Write,
        "order",
        "Charge the carriage on what comes back"
    ),
    route!(
        Post,
        "/admin/claims/{id}/outbound/items",
        Write,
        "order",
        "Add a replacement"
    ),
    route!(
        Delete,
        "/admin/claims/{id}/outbound/items/{action_id}",
        Write,
        "order",
        "Take a replacement off"
    ),
    route!(
        Post,
        "/admin/claims/{id}/outbound/shipping-method",
        Write,
        "order",
        "Charge the carriage on a replacement"
    ),
    route!(
        Post,
        "/admin/claims/{id}/request",
        Write,
        "order",
        "Settle the claim's open change"
    ),
    // Payments
    paged!("/admin/payments", "payment", "List payments"),
    route!(
        Get,
        "/admin/payments/{id}",
        View,
        "payment",
        "Fetch one payment"
    ),
    route!(
        Post,
        "/admin/payments/{id}/capture",
        Settle,
        "payment",
        "Take money that is being held"
    ),
    route!(
        Post,
        "/admin/payments/{id}/refund",
        Settle,
        "payment",
        "Give back money that was taken"
    ),
    // A provider's callback, and the two routes that make what it leaves
    // behind visible. The inbound one is neither a shopper's nor a back
    // office's: what authenticates it is a signature the host checks, because
    // the secret belongs to whoever configured the provider.
    Route {
        surface: Surface::Webhook,
        method: Method::Post,
        path: "/webhooks/payments/{provider}",
        action: Action::Write,
        domain: "payment",
        query: None,
        summary: "Receive a payment provider's callback",
    },
    paged!(
        "/admin/payment-webhooks",
        "payment",
        "List callbacks received and not yet acted on"
    ),
    route!(
        Post,
        "/admin/payment-webhooks/{id}/processed",
        Write,
        "payment",
        "Say a received callback has been acted on"
    ),
    route!(
        Post,
        "/admin/orders/{id}/refund-to-credit",
        Settle,
        "credit",
        "Refund an order onto the customer's balance instead of the card"
    ),
    route!(
        Get,
        "/admin/payments/payment-providers",
        View,
        "payment",
        "Which payment providers this shop has on"
    ),
    route!(
        Post,
        "/admin/payments/payment-providers",
        Write,
        "payment",
        "Register a payment provider"
    ),
    route!(
        Post,
        "/admin/payments/payment-providers/{id}/disable",
        Write,
        "payment",
        "Stop offering a payment provider without disturbing its payments"
    ),
    route!(
        Post,
        "/admin/payments/payment-providers/{id}/enable",
        Write,
        "payment",
        "Resume offering a disabled payment provider"
    ),
    // Payment collections
    route!(
        Post,
        "/admin/payment-collections",
        Write,
        "payment",
        "Open a payment collection"
    ),
    route!(
        Get,
        "/admin/payment-collections/{id}",
        View,
        "payment",
        "Fetch one payment collection"
    ),
    paged!(
        "/admin/payment-collections/{id}/payment-sessions",
        "payment",
        "List a collection's sessions"
    ),
    route!(
        Post,
        "/admin/payment-collections/{id}/payment-sessions",
        Write,
        "payment",
        "Open a session with a provider"
    ),
    // Reasons
    paged!("/admin/refund-reasons", "payment", "List refund reasons"),
    route!(
        Post,
        "/admin/refund-reasons",
        Write,
        "payment",
        "Add a refund reason"
    ),
    paged!("/admin/return-reasons", "order", "List return reasons"),
    route!(
        Post,
        "/admin/return-reasons",
        Write,
        "order",
        "Add a return reason"
    ),
    route!(
        Get,
        "/admin/return-reasons/{id}/translations",
        View,
        "order",
        "List a return reason's translations"
    ),
    route!(
        Post,
        "/admin/return-reasons/{id}/translations",
        Write,
        "order",
        "Write a return reason's translation into one locale"
    ),
    route!(
        Get,
        "/admin/return-reasons/{id}/translations/{locale}",
        View,
        "order",
        "Read a return reason in one locale, saying where it fell back"
    ),
    route!(
        Delete,
        "/admin/return-reasons/{id}/translations/{locale}",
        Delete,
        "order",
        "Drop a return reason's translation for one locale"
    ),
    // Fulfilments
    route!(
        Get,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}",
        View,
        "fulfilment",
        "Fetch one parcel, its contents and its labels"
    ),
    route!(
        Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/pack",
        Write,
        "fulfilment",
        "Mark a parcel packed"
    ),
    route!(
        Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/shipment",
        Write,
        "fulfilment",
        "Mark a parcel shipped, with its labels"
    ),
    route!(
        Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/mark-as-delivered",
        Write,
        "fulfilment",
        "Mark a parcel delivered"
    ),
    route!(
        Post,
        "/admin/orders/{id}/fulfillments/{fulfillment_id}/cancel",
        Settle,
        "fulfilment",
        "Cancel a parcel and put its stock back"
    ),
    // Fulfilment configuration
    paged!(
        "/admin/fulfillment-sets",
        "fulfilment",
        "List fulfilment sets"
    ),
    route!(
        Post,
        "/admin/fulfillment-sets",
        Write,
        "fulfilment",
        "Add a fulfilment set"
    ),
    route!(
        Delete,
        "/admin/fulfillment-sets/{id}",
        Delete,
        "fulfilment",
        "Remove a fulfilment set"
    ),
    route!(
        Get,
        "/admin/fulfillment-sets/{id}/service-zones",
        View,
        "fulfilment",
        "List a set's service zones"
    ),
    route!(
        Post,
        "/admin/fulfillment-sets/{id}/service-zones",
        Write,
        "fulfilment",
        "Add a service zone to a set"
    ),
    route!(
        Get,
        "/admin/fulfillment-providers",
        View,
        "fulfilment",
        "Which carriers this shop has on"
    ),
    route!(
        Post,
        "/admin/fulfillment-providers",
        Write,
        "fulfilment",
        "Register a carrier"
    ),
    route!(
        Post,
        "/admin/fulfillment-providers/{id}/disable",
        Write,
        "fulfilment",
        "Stop offering a carrier without deleting its shipments"
    ),
    route!(
        Post,
        "/admin/fulfillment-providers/{id}/enable",
        Write,
        "fulfilment",
        "Resume offering a dropped carrier"
    ),
    paged!(
        "/admin/shipping-options",
        "fulfilment",
        "List shipping options"
    ),
    route!(
        Post,
        "/admin/shipping-options",
        Write,
        "fulfilment",
        "Add a shipping option"
    ),
    route!(
        Get,
        "/admin/shipping-options/{id}",
        View,
        "fulfilment",
        "Fetch one shipping option"
    ),
    route!(
        Patch,
        "/admin/shipping-options/{id}",
        Write,
        "fulfilment",
        "Change a shipping option's name, price type or carrier"
    ),
    route!(
        Post,
        "/admin/shipping-options/{id}/rules",
        Write,
        "fulfilment",
        "Add a rule to a shipping option"
    ),
    route!(
        Get,
        "/admin/shipping-options/{id}/translations",
        View,
        "fulfilment",
        "List a shipping option's translations"
    ),
    route!(
        Post,
        "/admin/shipping-options/{id}/translations",
        Write,
        "fulfilment",
        "Write a shipping option's translation into one locale"
    ),
    route!(
        Get,
        "/admin/shipping-options/{id}/translations/{locale}",
        View,
        "fulfilment",
        "Read a shipping option in one locale, saying where it fell back"
    ),
    route!(
        Delete,
        "/admin/shipping-options/{id}/translations/{locale}",
        Delete,
        "fulfilment",
        "Drop a shipping option's translation for one locale"
    ),
    paged!(
        "/admin/shipping-profiles",
        "fulfilment",
        "List shipping profiles"
    ),
    route!(
        Post,
        "/admin/shipping-profiles",
        Write,
        "fulfilment",
        "Add a shipping profile"
    ),
    route!(
        Get,
        "/admin/shipping-profiles/{id}",
        View,
        "fulfilment",
        "Fetch one shipping profile"
    ),
    route!(
        Patch,
        "/admin/shipping-profiles/{id}",
        Write,
        "fulfilment",
        "Change a shipping profile"
    ),
    paged!(
        "/admin/shipping-option-types",
        "fulfilment",
        "List shipping option types"
    ),
    route!(
        Post,
        "/admin/shipping-option-types",
        Write,
        "fulfilment",
        "Add a shipping option type"
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_money_is_never_only_a_write() {
        for route in ROUTES {
            let moves_money = route.path.ends_with("/capture")
                || route.path.ends_with("/refund")
                || route.path.ends_with("/transactions") && route.method == Method::Post;

            if moves_money {
                assert_eq!(
                    route.action,
                    Action::Settle,
                    "{} moves money but asks for {:?}",
                    route.path,
                    route.action
                );
            }
        }
    }

    /// One exception, and it is the whole reason `Surface::Webhook` exists: a
    /// payment provider's callback belongs to this domain and to neither of
    /// the two audiences. It is checked here rather than exempted, so a second
    /// route quietly landing on that surface has to be argued for.
    #[test]
    fn every_route_is_on_the_admin_surface_but_the_provider_callback() {
        let mut elsewhere = Vec::new();
        for route in ROUTES {
            if route.surface != Surface::Admin {
                elsewhere.push((route.surface, route.path));
            }
        }

        assert_eq!(
            elsewhere,
            vec![(Surface::Webhook, "/webhooks/payments/{provider}")],
            "a route in this module is on a surface nobody wrote down here"
        );
    }
}
