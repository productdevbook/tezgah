//! Subscriptions, on both surfaces.
//!
//! The back office writes the plans, opens contracts, sees what is owed a
//! renewal and can bill or stop one by hand; a shopper sees their own and can
//! stop one at the end of the period they have paid for.
//!
//! # Renewing takes a pool
//!
//! [`renew`] takes the pool as well as the transaction, the way completing a
//! cart does: a renewal is a workflow and the engine opens one transaction per
//! step, so a step that finished stays finished when a later one does not.

use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::Result;
use crate::id::{
    AccountHolderId, CustomerId, OrderId, SellingPlanGroupId, SellingPlanId, SubscriptionId,
    VariantId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};
use crate::subscription;

use super::{Method, Route, Surface, signed_in};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl List {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match self.after.as_deref() {
            Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanGroupView {
    pub id: SellingPlanGroupId,
    pub name: String,
    pub description: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<subscription::SellingPlanGroup> for PlanGroupView {
    fn from(row: subscription::SellingPlanGroup) -> Self {
        PlanGroupView {
            id: row.id,
            name: row.name,
            description: row.description,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanView {
    pub id: SellingPlanId,
    pub selling_plan_group_id: SellingPlanGroupId,
    pub name: String,
    pub billing_interval_unit: String,
    pub billing_interval_count: i32,
    pub delivery_interval_unit: Option<String>,
    pub delivery_interval_count: Option<i32>,
    pub discount_kind: Option<String>,
    pub discount_value: Option<rust_decimal::Decimal>,
    pub currency_code: Option<String>,
    pub applies_to: String,
    pub max_cycles: Option<i32>,
}

impl From<subscription::SellingPlan> for PlanView {
    fn from(row: subscription::SellingPlan) -> Self {
        PlanView {
            id: row.id,
            selling_plan_group_id: row.selling_plan_group_id,
            name: row.name,
            billing_interval_unit: row.billing_interval_unit,
            billing_interval_count: row.billing_interval_count,
            delivery_interval_unit: row.delivery_interval_unit,
            delivery_interval_count: row.delivery_interval_count,
            discount_kind: row.discount_kind,
            discount_value: row.discount_value,
            currency_code: row.currency_code,
            applies_to: row.applies_to,
            max_cycles: row.max_cycles,
        }
    }
}

/// A contract as it leaves the building. No `payment_method_reference`: it is a
/// reference to somebody's instrument and a list is read far more often than a
/// card is charged.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SubscriptionView {
    pub id: SubscriptionId,
    pub customer_id: CustomerId,
    pub selling_plan_id: SellingPlanId,
    pub currency_code: String,
    pub status: String,
    pub next_billing_at: chrono::DateTime<chrono::Utc>,
    pub current_period_start: chrono::DateTime<chrono::Utc>,
    pub current_period_end: chrono::DateTime<chrono::Utc>,
    pub cycle: i32,
    pub cancel_at_period_end: bool,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub dunning_attempts: i32,
}

impl From<subscription::Subscription> for SubscriptionView {
    fn from(row: subscription::Subscription) -> Self {
        SubscriptionView {
            id: row.id,
            customer_id: row.customer_id,
            selling_plan_id: row.selling_plan_id,
            currency_code: row.currency_code,
            status: row.status,
            next_billing_at: row.next_billing_at,
            current_period_start: row.current_period_start,
            current_period_end: row.current_period_end,
            cycle: row.cycle,
            cancel_at_period_end: row.cancel_at_period_end,
            ended_at: row.ended_at,
            dunning_attempts: row.dunning_attempts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineView {
    pub variant_id: VariantId,
    pub title: Option<String>,
    pub quantity: i32,
    pub unit_price: rust_decimal::Decimal,
    pub currency_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractView {
    #[serde(flatten)]
    pub subscription: SubscriptionView,
    pub lines: Vec<LineView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventView {
    pub kind: String,
    pub payload: Option<serde_json::Value>,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenewedView {
    pub order_id: Option<OrderId>,
    pub cycle: i32,
    pub declined: bool,
    pub cancelled: bool,
    pub state: String,
    pub failure: Option<String>,
}

// ---------------------------------------------------------------------------
// Admin
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePlanGroup {
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn create_plan_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreatePlanGroup,
) -> Result<PlanGroupView> {
    Ok(PlanGroupView::from(
        subscription::create_plan_group(
            tx,
            ctx,
            subscription::NewPlanGroup {
                name: body.name,
                description: body.description,
                metadata: body.metadata,
            },
        )
        .await?,
    ))
}

pub async fn list_plan_groups(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<PlanGroupView>> {
    let page = subscription::plan_groups(tx, ctx, query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(PlanGroupView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePlan {
    pub name: String,
    pub billing_interval_unit: String,
    pub billing_interval_count: i32,
    pub delivery_interval_unit: Option<String>,
    pub delivery_interval_count: Option<i32>,
    pub prepaid_cycles: Option<i32>,
    pub min_cycles: Option<i32>,
    pub max_cycles: Option<i32>,
    pub discount_kind: Option<String>,
    pub discount_value: Option<rust_decimal::Decimal>,
    pub currency_code: Option<String>,
    pub applies_to: Option<String>,
    pub dunning_max_attempts: Option<i32>,
    pub dunning_interval_hours: Option<i32>,
    pub position: Option<i32>,
}

pub async fn create_plan(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: SellingPlanGroupId,
    body: CreatePlan,
) -> Result<PlanView> {
    let new = subscription::NewPlan {
        name: body.name,
        billing_interval_unit: body.billing_interval_unit,
        billing_interval_count: body.billing_interval_count,
        delivery_interval_unit: body.delivery_interval_unit,
        delivery_interval_count: body.delivery_interval_count,
        prepaid_cycles: body.prepaid_cycles,
        min_cycles: body.min_cycles,
        max_cycles: body.max_cycles,
        discount_kind: body.discount_kind,
        discount_value: body.discount_value,
        currency_code: body.currency_code,
        applies_to: body.applies_to,
        dunning_max_attempts: body.dunning_max_attempts,
        dunning_interval_hours: body.dunning_interval_hours,
        position: body.position,
    };

    Ok(PlanView::from(
        subscription::create_plan(tx, ctx, group_id, new).await?,
    ))
}

pub async fn get_plan(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: SellingPlanId) -> Result<PlanView> {
    Ok(PlanView::from(subscription::plan(tx, ctx, id).await?))
}

pub async fn list_plans(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: SellingPlanGroupId,
    query: List,
) -> Result<Page<PlanView>> {
    let page = subscription::plans(tx, ctx, group_id, query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(PlanView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachVariant {
    pub variant_id: VariantId,
}

pub async fn attach_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    plan_id: SellingPlanId,
    body: AttachVariant,
) -> Result<()> {
    subscription::attach_variant(tx, ctx, plan_id, body.variant_id).await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateLine {
    pub variant_id: VariantId,
    pub title: Option<String>,
    pub quantity: i32,
    pub unit_price: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateSubscription {
    pub customer_id: CustomerId,
    pub selling_plan_id: SellingPlanId,
    pub currency_code: String,
    pub region_id: Option<crate::id::RegionId>,
    pub sales_channel_id: Option<crate::id::SalesChannelId>,
    pub account_holder_id: Option<crate::id::AccountHolderId>,
    pub payment_method_reference: Option<String>,
    pub mandate_reference: Option<String>,
    pub mandate_accepted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub shipping_address_id: Option<crate::id::AddressId>,
    pub billing_address_id: Option<crate::id::AddressId>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub lines: Vec<CreateLine>,
}

pub async fn create_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreateSubscription,
) -> Result<SubscriptionView> {
    let currency = Currency::parse(&body.currency_code)?;

    let new = subscription::NewSubscription {
        customer_id: body.customer_id,
        selling_plan_id: body.selling_plan_id,
        currency,
        region_id: body.region_id,
        sales_channel_id: body.sales_channel_id,
        account_holder_id: body.account_holder_id,
        payment_method_reference: body.payment_method_reference,
        mandate_reference: body.mandate_reference,
        mandate_accepted_at: body.mandate_accepted_at,
        shipping_address_id: body.shipping_address_id,
        billing_address_id: body.billing_address_id,
        starts_at: body.starts_at,
        lines: body
            .lines
            .into_iter()
            .map(|line| subscription::NewLine {
                variant_id: line.variant_id,
                title: line.title,
                quantity: line.quantity,
                unit_price: Money::new(line.unit_price, currency),
            })
            .collect(),
    };

    Ok(SubscriptionView::from(
        subscription::create(tx, ctx, new).await?,
    ))
}

pub async fn list_subscriptions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<SubscriptionView>> {
    let page = subscription::list(tx, ctx, None, query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(SubscriptionView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListDue {
    /// The moment to ask about, which is the host's clock rather than tezgah's.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

/// What is owed a renewal. The screen a shop looks at when it wants to know
/// what its own cron is about to do.
pub async fn list_due(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListDue,
) -> Result<Page<SubscriptionView>> {
    let paging = List {
        after: query.after,
        limit: query.limit,
    }
    .paging()?;

    let page = subscription::due(tx, ctx, query.at.unwrap_or_else(|| ctx.now()), paging).await?;

    Ok(Page {
        items: page.items.into_iter().map(SubscriptionView::from).collect(),
        next: page.next,
    })
}

pub async fn get_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<ContractView> {
    let contract = subscription::get(tx, ctx, id).await?;
    let lines = subscription::lines(tx, ctx, id).await?;

    Ok(ContractView {
        subscription: SubscriptionView::from(contract),
        lines: lines
            .into_iter()
            .map(|line| LineView {
                variant_id: line.variant_id,
                title: line.title,
                quantity: line.quantity,
                unit_price: line.unit_price,
                currency_code: line.currency_code,
            })
            .collect(),
    })
}

pub async fn list_events(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    query: List,
) -> Result<Page<EventView>> {
    let page = subscription::events(tx, ctx, id, query.paging()?).await?;

    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(|row| EventView {
                kind: row.kind,
                payload: row.payload,
                at: row.at,
            })
            .collect(),
        next: page.next,
    })
}

/// Bills a contract by hand, for the cycle it is owed.
///
/// The same call the host's cron makes, and the same transaction key, so doing
/// it here while the cron is doing it there is one renewal rather than two.
pub async fn renew(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    how: &subscription::Renewals,
    pool: &PgPool,
) -> Result<RenewedView> {
    subscription::get(tx, ctx, id).await?;

    let renewed = how.renew(pool, ctx, id).await?;

    // No run means nothing was owed yet — `Renewals::renew` refused rather
    // than billing a cycle before its `next_billing_at`.
    let state = renewed
        .run
        .as_ref()
        .map(|run| format!("{:?}", run.state).to_lowercase())
        .unwrap_or_else(|| "not_due".to_string());

    Ok(RenewedView {
        order_id: renewed.order_id,
        cycle: renewed.cycle,
        declined: renewed.declined,
        cancelled: renewed.cancelled,
        state,
        failure: renewed.run.and_then(|run| run.failure),
    })
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cancel {
    /// Left out, a contract stops at the end of the period already paid for.
    pub immediately: Option<bool>,
    pub reason: Option<String>,
}

pub async fn cancel_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Cancel,
) -> Result<SubscriptionView> {
    Ok(SubscriptionView::from(
        subscription::cancel(
            tx,
            ctx,
            id,
            !body.immediately.unwrap_or(false),
            body.reason.as_deref(),
        )
        .await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pause {
    pub until: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn pause_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Pause,
) -> Result<SubscriptionView> {
    Ok(SubscriptionView::from(
        subscription::pause(tx, ctx, id, body.until).await?,
    ))
}

pub async fn resume_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<SubscriptionView> {
    Ok(SubscriptionView::from(
        subscription::resume(tx, ctx, id).await?,
    ))
}

pub async fn skip_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<SubscriptionView> {
    Ok(SubscriptionView::from(
        subscription::skip_next(tx, ctx, id).await?,
    ))
}

/// What replacing a contract's card takes on either surface: the new
/// reference and the instrument under it. `subscription::repoint_card` checks
/// the reference is this contract's own customer's.
///
/// `mandate_reference` is the fresh consent for this instrument, if the
/// caller collected one — left out, `subscription::repoint_card` clears the
/// old mandate rather than carrying it onto a card it was never given for.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepointCard {
    pub account_holder_id: AccountHolderId,
    pub payment_method_reference: String,
    pub mandate_reference: Option<String>,
}

/// Points a contract at a different saved card, by hand — support fixing what
/// a customer's own retry could not, or could not wait for.
pub async fn repoint_subscription_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: RepointCard,
    how: &subscription::Renewals,
    pool: &PgPool,
) -> Result<SubscriptionView> {
    subscription::get(tx, ctx, id).await?;

    Ok(SubscriptionView::from(
        subscription::repoint_card(
            pool,
            ctx,
            id,
            body.account_holder_id,
            body.payment_method_reference,
            body.mandate_reference,
            how,
        )
        .await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SwapLine {
    pub variant_id: VariantId,
    pub title: Option<String>,
    pub quantity: i32,
    pub unit_price: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Swap {
    pub lines: Vec<SwapLine>,
}

pub async fn swap_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Swap,
) -> Result<SubscriptionView> {
    let contract = subscription::get(tx, ctx, id).await?;
    let currency = contract.currency()?;

    let lines = body
        .lines
        .into_iter()
        .map(|line| subscription::NewLine {
            variant_id: line.variant_id,
            title: line.title,
            quantity: line.quantity,
            unit_price: Money::new(line.unit_price, currency),
        })
        .collect();

    Ok(SubscriptionView::from(
        subscription::swap(tx, ctx, id, lines).await?,
    ))
}

/// What is owed a delivery, on a prepaid term. The delivery counterpart of
/// [`list_due`].
pub async fn list_due_deliveries(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListDue,
) -> Result<Page<SubscriptionView>> {
    let paging = List {
        after: query.after,
        limit: query.limit,
    }
    .paging()?;

    let page = subscription::due_deliveries(tx, ctx, query.at.unwrap_or_else(|| ctx.now()), paging)
        .await?;

    Ok(Page {
        items: page.items.into_iter().map(SubscriptionView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deliver {
    pub location_id: crate::id::StockLocationId,
}

/// Ships one delivery a prepaid term already paid for, by hand.
pub async fn deliver_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Deliver,
) -> Result<OrderId> {
    subscription::deliver(tx, ctx, id, body.location_id).await
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

pub async fn my_subscriptions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: List,
) -> Result<Page<SubscriptionView>> {
    let customer = signed_in(ctx)?;
    let page = subscription::list(tx, ctx, Some(customer), query.paging()?).await?;

    Ok(Page {
        items: page.items.into_iter().map(SubscriptionView::from).collect(),
        next: page.next,
    })
}

/// A shopper stopping their own contract. At the end of the period they have
/// paid for and never in the middle of one: what a mid-period stop is worth
/// back is proration, and proration is not here yet.
pub async fn cancel_my_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Cancel,
) -> Result<SubscriptionView> {
    let customer = signed_in(ctx)?;
    let contract = subscription::get(tx, ctx, id).await?;
    if contract.customer_id != customer {
        return Err(crate::Error::denied());
    }

    Ok(SubscriptionView::from(
        subscription::cancel(tx, ctx, id, true, body.reason.as_deref()).await?,
    ))
}

async fn owned(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: SubscriptionId) -> Result<()> {
    let customer = signed_in(ctx)?;
    let contract = subscription::get(tx, ctx, id).await?;
    if contract.customer_id != customer {
        return Err(crate::Error::denied());
    }
    Ok(())
}

pub async fn pause_my_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: Pause,
) -> Result<SubscriptionView> {
    owned(tx, ctx, id).await?;
    Ok(SubscriptionView::from(
        subscription::pause(tx, ctx, id, body.until).await?,
    ))
}

pub async fn resume_my_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<SubscriptionView> {
    owned(tx, ctx, id).await?;
    Ok(SubscriptionView::from(
        subscription::resume(tx, ctx, id).await?,
    ))
}

pub async fn skip_my_subscription(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<SubscriptionView> {
    owned(tx, ctx, id).await?;
    Ok(SubscriptionView::from(
        subscription::skip_next(tx, ctx, id).await?,
    ))
}

/// A shopper pointing their own contract at a card they just saved through
/// [`save_my_account_holder`](super::store::save_my_account_holder) —
/// tokenised with the provider directly, unchanged by any of this. Own
/// subscription and own account holder are both asked about:
/// [`owned`] here, [`subscription::repoint_card`] for the reference itself.
pub async fn repoint_my_subscription_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    body: RepointCard,
    how: &subscription::Renewals,
    pool: &PgPool,
) -> Result<SubscriptionView> {
    owned(tx, ctx, id).await?;

    Ok(SubscriptionView::from(
        subscription::repoint_card(
            pool,
            ctx,
            id,
            body.account_holder_id,
            body.payment_method_reference,
            body.mandate_reference,
            how,
        )
        .await?,
    ))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/selling-plan-groups",
        action: Action::Write,
        domain: "subscription",
        summary: "Write a group of selling plans",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/selling-plan-groups",
        action: Action::View,
        domain: "subscription",
        summary: "List the groups of selling plans",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/selling-plan-groups/{id}/plans",
        action: Action::Write,
        domain: "subscription",
        summary: "Add a plan to a group",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/selling-plan-groups/{id}/plans",
        action: Action::View,
        domain: "subscription",
        summary: "List a group's plans",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/selling-plans/{id}",
        action: Action::View,
        domain: "subscription",
        summary: "One plan and how often it bills",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/selling-plans/{id}/variants",
        action: Action::Write,
        domain: "subscription",
        summary: "Offer a variant on this plan",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions",
        action: Action::Write,
        domain: "subscription",
        summary: "Open a contract",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/subscriptions",
        action: Action::View,
        domain: "subscription",
        summary: "List the contracts",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/subscriptions/due",
        action: Action::View,
        domain: "subscription",
        summary: "What is owed a renewal",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/subscriptions/{id}",
        action: Action::View,
        domain: "subscription",
        summary: "One contract and what it is for",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/subscriptions/{id}/events",
        action: Action::View,
        domain: "subscription",
        summary: "What has happened to this contract",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/renew",
        action: Action::Settle,
        domain: "subscription",
        summary: "Bill this contract for the cycle it is owed",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/cancel",
        action: Action::Write,
        domain: "subscription",
        summary: "Stop a contract",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/pause",
        action: Action::Write,
        domain: "subscription",
        summary: "Stop a contract renewing until it is resumed",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/resume",
        action: Action::Write,
        domain: "subscription",
        summary: "Start a paused contract renewing again",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/skip",
        action: Action::Write,
        domain: "subscription",
        summary: "Pass over the next period without billing it",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/swap",
        action: Action::Write,
        domain: "subscription",
        summary: "Swap what a contract is for",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/card",
        action: Action::Write,
        domain: "subscription",
        summary: "Point a contract at a different saved card",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/subscriptions/due-deliveries",
        action: Action::View,
        domain: "subscription",
        summary: "What is owed a delivery on a prepaid term",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/subscriptions/{id}/deliver",
        action: Action::Write,
        domain: "subscription",
        summary: "Ship one delivery a prepaid term already paid for",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/subscriptions",
        action: Action::View,
        domain: "subscription",
        summary: "What I have subscribed to",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/subscriptions/{id}/cancel",
        action: Action::Write,
        domain: "subscription",
        summary: "Stop mine at the end of this period",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/subscriptions/{id}/pause",
        action: Action::Write,
        domain: "subscription",
        summary: "Stop mine renewing until I resume it",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/subscriptions/{id}/resume",
        action: Action::Write,
        domain: "subscription",
        summary: "Start mine renewing again",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/subscriptions/{id}/skip",
        action: Action::Write,
        domain: "subscription",
        summary: "Pass over my next period without billing it",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/subscriptions/{id}/card",
        action: Action::Write,
        domain: "subscription",
        summary: "Point mine at a different saved card",
    },
];
