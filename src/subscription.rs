//! Selling the same thing again next month, and what happens when the card says no.
//!
//! Three objects. A [`SellingPlan`] is catalogue: how often a variant is billed
//! and how often it is delivered, which are two policies and not one. A
//! [`Subscription`] is the contract that comes out of one — long-lived, with its
//! own versioned lines, its own addresses and the two opaque references that let
//! a stored instrument be charged. The orders come afterwards, one per cycle,
//! and `subscription_order` is what makes "how much has this subscriber paid"
//! answerable.
//!
//! # Nothing here is a scheduler
//!
//! tezgah does not sleep in a thread. [`due`] answers which contracts are owed a
//! renewal at a given moment and [`Renewals::renew`] renews one; the host's cron
//! polls the first and calls the second, exactly as it already does with
//! `inventory::expire_reservations`. The workflow runner's `work()` claims runs
//! that exist — it creates none, so it renews nothing on its own.
//!
//! # The same cycle cannot be billed twice
//!
//! A renewal runs under the transaction key `subscription:{id}:{cycle}`, so two
//! schedulers firing at once, a cron overlapping itself and a host retrying its
//! own job all collapse onto one run. Behind that, `subscription_order` is
//! unique on `(scope, subscription_id, cycle)`, so even a run under another key
//! cannot write a second order for a period already billed.
//!
//! # A renewal runs as [`Actor::System`]
//!
//! There is nobody in a browser. A host whose `Authorizer` denies `System` stops
//! every renewal in the shop, and it will stop them quietly, because the only
//! caller is a cron nobody watches.
//!
//! # The payment boundary
//!
//! tezgah never holds a card. It holds `account_holder.external_id` and
//! `subscription.payment_method_reference` — two strings a provider issued and
//! nothing here interprets — and the mandate reference that says the shopper
//! agreed to be charged again. Charging is
//! [`RecurringProvider::authorize_stored`](crate::payment::RecurringProvider);
//! what happens when it declines is dunning, which is commerce and therefore
//! tezgah's.
//!
//! # What is not here yet
//!
//! Pause, resume, skip and swap, and prepaid terms with their proration. The
//! schema carries `paused_until`, `prepaid_cycles` and `min_cycles` for them;
//! nothing writes those columns today.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Days, Months, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    AccountHolderId, AddressId, CustomerId, OrderId, PaymentCollectionId, PaymentId,
    PaymentSessionId, RegionId, ReservationId, SalesChannelId, SellingPlanGroupId, SellingPlanId,
    StockLocationId, SubscriptionEventId, SubscriptionId, SubscriptionLineId, SubscriptionOrderId,
    VariantId,
};
use crate::money::{Currency, Money};
use crate::order::{self, NewOrder, NewOrderLine, NewTaxLine, OrderAddress, TaxSnapshot};
use crate::page::{Cursor, Page, Paging};
use crate::payment::{
    self, AuthorizationStatus, Authorized, NewCollection, NewSession, RecurringProvider,
    StoredChargeRequest,
};
use crate::ports::{Action, AuditEntry, Ctx, Event, JobSpec, Permit, Resource, Tx};
use crate::workflow::{self, Failure, Outcome, Step, Workflow};
use crate::{inventory, pricing, store, tax};

/// Most lines one contract may carry. A box of six things is a contract; a
/// thousand is an import that went wrong.
pub const MAX_LINES: i64 = 100;

/// What the job a declined charge enqueues is called. A host runs it by calling
/// [`Renewals::renew`] again with the same contract.
pub const DUNNING_JOB: &str = "subscription.dunning";

/// The step whose failure means the card said no rather than the shop being out
/// of stock. Matched on the step's name, which the engine puts in front of the
/// failure it stores — never on the text of the error.
const CHARGE_STEP: &str = "charge_stored_method";

const GROUP_COLUMNS: &str = "id, name, description, metadata, deleted_at, created_at";

const PLAN_COLUMNS: &str = "id, selling_plan_group_id, name, billing_interval_unit, \
     billing_interval_count, delivery_interval_unit, delivery_interval_count, prepaid_cycles, \
     min_cycles, max_cycles, discount_kind, discount_value, currency_code, applies_to, \
     dunning_max_attempts, dunning_interval_hours, position, deleted_at, created_at";

const SUBSCRIPTION_COLUMNS: &str = "id, customer_id, selling_plan_id, currency_code, region_id, \
     sales_channel_id, status, account_holder_id, payment_method_reference, mandate_reference, \
     mandate_accepted_at, shipping_address_id, billing_address_id, next_billing_at, \
     current_period_start, current_period_end, cycle, line_version, cancel_at_period_end, \
     paused_until, ended_at, dunning_attempts, created_at";

const LINE_COLUMNS: &str =
    "id, subscription_id, version, variant_id, title, quantity, unit_price, currency_code";

const EVENT_COLUMNS: &str = "id, subscription_id, kind, payload, at";

// ---------------------------------------------------------------------------
// What the rows look like
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SellingPlanGroup {
    pub id: SellingPlanGroupId,
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<Value>,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewPlanGroup {
    pub name: String,
    pub description: Option<String>,
    pub metadata: Option<Value>,
}

/// How often something is billed, and how often it arrives.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SellingPlan {
    pub id: SellingPlanId,
    pub selling_plan_group_id: SellingPlanGroupId,
    pub name: String,
    pub billing_interval_unit: String,
    pub billing_interval_count: i32,
    pub delivery_interval_unit: Option<String>,
    pub delivery_interval_count: Option<i32>,
    pub prepaid_cycles: Option<i32>,
    pub min_cycles: Option<i32>,
    pub max_cycles: Option<i32>,
    pub discount_kind: Option<String>,
    pub discount_value: Option<Decimal>,
    pub currency_code: Option<String>,
    pub applies_to: String,
    pub dunning_max_attempts: i32,
    pub dunning_interval_hours: i32,
    pub position: i32,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl SellingPlan {
    /// Whether the plan's own discount is owed on a renewal, as opposed to only
    /// on the order that opened the contract.
    pub fn discounts_renewals(&self) -> bool {
        self.applies_to == "renewals" || self.applies_to == "every_order"
    }
}

#[derive(Debug, Clone, Default)]
pub struct NewPlan {
    pub name: String,
    pub billing_interval_unit: String,
    pub billing_interval_count: i32,
    pub delivery_interval_unit: Option<String>,
    pub delivery_interval_count: Option<i32>,
    pub prepaid_cycles: Option<i32>,
    pub min_cycles: Option<i32>,
    pub max_cycles: Option<i32>,
    pub discount_kind: Option<String>,
    pub discount_value: Option<Decimal>,
    pub currency_code: Option<String>,
    pub applies_to: Option<String>,
    pub dunning_max_attempts: Option<i32>,
    pub dunning_interval_hours: Option<i32>,
    pub position: Option<i32>,
}

/// The contract.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Subscription {
    pub id: SubscriptionId,
    pub customer_id: CustomerId,
    pub selling_plan_id: SellingPlanId,
    pub currency_code: String,
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    pub status: String,
    /// Named rather than looked up: a customer may hold several accounts with
    /// one provider, and an off-session charge against the wrong one is a
    /// decline the customer never sees.
    pub account_holder_id: Option<AccountHolderId>,
    pub payment_method_reference: Option<String>,
    pub mandate_reference: Option<String>,
    pub mandate_accepted_at: Option<DateTime<Utc>>,
    pub shipping_address_id: Option<AddressId>,
    pub billing_address_id: Option<AddressId>,
    pub next_billing_at: DateTime<Utc>,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub cycle: i32,
    pub line_version: i32,
    pub cancel_at_period_end: bool,
    pub paused_until: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub dunning_attempts: i32,
    pub created_at: DateTime<Utc>,
}

impl Subscription {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    /// Whether another cycle may still be billed.
    pub fn is_live(&self) -> bool {
        self.status == "active" || self.status == "past_due"
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SubscriptionLine {
    pub id: SubscriptionLineId,
    pub subscription_id: SubscriptionId,
    pub version: i32,
    pub variant_id: VariantId,
    pub title: Option<String>,
    pub quantity: i32,
    pub unit_price: Decimal,
    pub currency_code: String,
}

#[derive(Debug, Clone)]
pub struct NewLine {
    pub variant_id: VariantId,
    pub title: Option<String>,
    pub quantity: i32,
    pub unit_price: Money,
}

#[derive(Debug, Clone)]
pub struct NewSubscription {
    pub customer_id: CustomerId,
    pub selling_plan_id: SellingPlanId,
    pub currency: Currency,
    pub region_id: Option<RegionId>,
    pub sales_channel_id: Option<SalesChannelId>,
    pub account_holder_id: Option<AccountHolderId>,
    pub payment_method_reference: Option<String>,
    pub mandate_reference: Option<String>,
    pub mandate_accepted_at: Option<DateTime<Utc>>,
    pub shipping_address_id: Option<AddressId>,
    pub billing_address_id: Option<AddressId>,
    /// When the first period starts. The first renewal falls due at the end of
    /// it, which is what the plan's billing interval decides.
    pub starts_at: Option<DateTime<Utc>>,
    pub lines: Vec<NewLine>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SubscriptionEvent {
    pub id: SubscriptionEventId,
    pub subscription_id: SubscriptionId,
    pub kind: String,
    pub payload: Option<Value>,
    pub at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// The catalogue side
// ---------------------------------------------------------------------------

/// The units a billing or delivery interval may be counted in.
fn interval_unit(text: &str) -> Result<&'static str> {
    match text {
        "day" => Ok("day"),
        "week" => Ok("week"),
        "month" => Ok("month"),
        "year" => Ok("year"),
        _ => Err(Error::invalid(
            "an interval is counted in days, weeks, months or years",
        )),
    }
}

fn applies_to(text: &str) -> Result<&'static str> {
    match text {
        "first_order" => Ok("first_order"),
        "renewals" => Ok("renewals"),
        "every_order" => Ok("every_order"),
        _ => Err(Error::invalid(
            "a plan's discount applies to the first order, to renewals or to every order",
        )),
    }
}

fn discount_kind(text: &str) -> Result<&'static str> {
    match text {
        "percentage" => Ok("percentage"),
        "fixed" => Ok("fixed"),
        _ => Err(Error::invalid("a discount is a percentage or an amount")),
    }
}

/// Puts a group of plans on the shelf, or renames the one already under that
/// name.
///
/// Catalogue rather than contract: whoever may say what is sold may say how
/// often it recurs.
pub async fn create_plan_group(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewPlanGroup,
) -> Result<SellingPlanGroup> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let name = new.name.trim();
    if name.is_empty() {
        return Err(Error::invalid("a group of selling plans needs a name"));
    }

    let group = sqlx::query_as::<_, SellingPlanGroup>(&format!(
        "insert into selling_plan_group (id, scope, name, description, metadata)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, name) where deleted_at is null
         do update set description = excluded.description, metadata = excluded.metadata
         returning {GROUP_COLUMNS}"
    ))
    .bind(SellingPlanGroupId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(name)
    .bind(new.description)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "selling_plan_group",
            entity_id: group.id.as_uuid(),
            summary: serde_json::json!({ "name": group.name }),
        },
    )
    .await?;

    Ok(group)
}

pub async fn plan_groups(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<SellingPlanGroup>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, SellingPlanGroup>(&format!(
        "select {GROUP_COLUMNS} from selling_plan_group
         where scope = $1 and deleted_at is null
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

pub async fn create_plan(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: SellingPlanGroupId,
    new: NewPlan,
) -> Result<SellingPlan> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let name = new.name.trim();
    if name.is_empty() {
        return Err(Error::invalid("a selling plan needs a name"));
    }
    let billing_unit = interval_unit(&new.billing_interval_unit)?;
    if new.billing_interval_count <= 0 {
        return Err(Error::invalid("a billing interval of nothing never bills"));
    }
    let delivery_unit = match new.delivery_interval_unit.as_deref() {
        Some(text) => Some(interval_unit(text)?),
        None => None,
    };
    let kind = match new.discount_kind.as_deref() {
        Some(text) => Some(discount_kind(text)?),
        None => None,
    };
    if kind == Some("fixed") && new.currency_code.is_none() {
        return Err(Error::invalid(
            "a discount of an amount has to say which currency",
        ));
    }
    let applies = applies_to(new.applies_to.as_deref().unwrap_or("renewals"))?;

    let plan = sqlx::query_as::<_, SellingPlan>(&format!(
        "insert into selling_plan
             (id, scope, selling_plan_group_id, name, billing_interval_unit,
              billing_interval_count, delivery_interval_unit, delivery_interval_count,
              prepaid_cycles, min_cycles, max_cycles, discount_kind, discount_value,
              currency_code, applies_to, dunning_max_attempts, dunning_interval_hours, position)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15,
                 coalesce($16, 3), coalesce($17, 72), coalesce($18, 0))
         returning {PLAN_COLUMNS}"
    ))
    .bind(SellingPlanId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
    .bind(name)
    .bind(billing_unit)
    .bind(new.billing_interval_count)
    .bind(delivery_unit)
    .bind(new.delivery_interval_count)
    .bind(new.prepaid_cycles)
    .bind(new.min_cycles)
    .bind(new.max_cycles)
    .bind(kind)
    .bind(new.discount_value)
    .bind(new.currency_code)
    .bind(applies)
    .bind(new.dunning_max_attempts)
    .bind(new.dunning_interval_hours)
    .bind(new.position)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "selling_plan",
            entity_id: plan.id.as_uuid(),
            summary: serde_json::json!({ "group": group_id.to_string(), "name": plan.name }),
        },
    )
    .await?;

    Ok(plan)
}

pub async fn plans(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    group_id: SellingPlanGroupId,
    paging: Paging,
) -> Result<Page<SellingPlan>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    let rows = sqlx::query_as::<_, SellingPlan>(&format!(
        "select {PLAN_COLUMNS} from selling_plan
         where scope = $1 and selling_plan_group_id = $2 and deleted_at is null
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(group_id.as_uuid())
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

pub async fn plan(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: SellingPlanId) -> Result<SellingPlan> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    sqlx::query_as::<_, SellingPlan>(&format!(
        "select {PLAN_COLUMNS} from selling_plan where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("selling plan"))
}

/// Says a variant may be bought on this plan. Idempotent, so a second offer of
/// the same pairing changes nothing.
pub async fn attach_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    plan_id: SellingPlanId,
    variant_id: VariantId,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    sqlx::query(
        "insert into selling_plan_variant (id, scope, selling_plan_id, variant_id)
         values ($1, $2, $3, $4)
         on conflict (scope, selling_plan_id, variant_id) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(plan_id.as_uuid())
    .bind(variant_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "selling_plan_variant",
            entity_id: plan_id.as_uuid(),
            summary: serde_json::json!({ "variant": variant_id.to_string() }),
        },
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// The contract
// ---------------------------------------------------------------------------

/// What a period of this plan is: the moment it ends, given where it starts.
fn advance(at: DateTime<Utc>, unit: &str, count: i32) -> Result<DateTime<Utc>> {
    let count = u32::try_from(count).map_err(|_| Error::invalid("an interval counts upward"))?;
    let moved = match unit {
        "day" => at.checked_add_days(Days::new(u64::from(count))),
        "week" => at.checked_add_days(Days::new(u64::from(count) * 7)),
        "month" => at.checked_add_months(Months::new(count)),
        "year" => at.checked_add_months(Months::new(count.saturating_mul(12))),
        _ => return Err(Error::bug("a plan holds an interval nothing writes")),
    };

    moved.ok_or_else(|| Error::invalid("that period runs off the end of the calendar"))
}

/// Opens a contract, with its first period already running.
pub async fn create(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewSubscription) -> Result<Subscription> {
    let id = SubscriptionId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Subscription {
            id: Some(id.as_uuid()),
            customer: Some(new.customer_id.as_uuid()),
        },
    )?;

    if new.lines.is_empty() {
        return Err(Error::invalid("a contract needs something on it"));
    }
    if new.lines.len() as i64 > MAX_LINES {
        return Err(Error::invalid(
            "that is more lines than a contract may hold",
        ));
    }
    for line in &new.lines {
        if line.quantity <= 0 {
            return Err(Error::invalid("a line needs a quantity of at least one"));
        }
        if line.unit_price.is_negative() {
            return Err(Error::invalid("a price cannot be negative"));
        }
        if line.unit_price.currency != new.currency {
            return Err(Error::invalid("a line is priced in another currency"));
        }
    }

    let plan = plan(tx, ctx, new.selling_plan_id).await?;
    let starts_at = new.starts_at.unwrap_or_else(|| ctx.now());
    let ends_at = advance(
        starts_at,
        &plan.billing_interval_unit,
        plan.billing_interval_count,
    )?;

    let contract = sqlx::query_as::<_, Subscription>(&format!(
        "insert into subscription
             (id, scope, customer_id, selling_plan_id, currency_code, region_id,
              sales_channel_id, account_holder_id, payment_method_reference, mandate_reference,
              mandate_accepted_at, shipping_address_id, billing_address_id, next_billing_at,
              current_period_start, current_period_end)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
         returning {SUBSCRIPTION_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.customer_id.as_uuid())
    .bind(new.selling_plan_id.as_uuid())
    .bind(new.currency.as_str())
    .bind(new.region_id.map(RegionId::as_uuid))
    .bind(new.sales_channel_id.map(SalesChannelId::as_uuid))
    .bind(new.account_holder_id.map(AccountHolderId::as_uuid))
    .bind(new.payment_method_reference)
    .bind(new.mandate_reference)
    .bind(new.mandate_accepted_at)
    .bind(new.shipping_address_id.map(AddressId::as_uuid))
    .bind(new.billing_address_id.map(AddressId::as_uuid))
    .bind(ends_at)
    .bind(starts_at)
    .bind(ends_at)
    .fetch_one(&mut **tx)
    .await?;

    for line in &new.lines {
        write_line(tx, ctx, id, 1, line).await?;
    }

    record(
        tx,
        ctx,
        id,
        "created",
        serde_json::json!({
            "plan": new.selling_plan_id,
            "next_billing_at": ends_at,
        }),
    )
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "subscription",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "customer": new.customer_id.to_string() }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "subscription.created",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "customer": new.customer_id,
                "next_billing_at": ends_at,
            }),
        },
    )
    .await?;

    Ok(contract)
}

async fn write_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    version: i32,
    line: &NewLine,
) -> Result<SubscriptionLine> {
    Ok(sqlx::query_as::<_, SubscriptionLine>(&format!(
        "insert into subscription_line
             (id, scope, subscription_id, version, variant_id, title, quantity, unit_price,
              currency_code)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         returning {LINE_COLUMNS}"
    ))
    .bind(SubscriptionLineId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(version)
    .bind(line.variant_id.as_uuid())
    .bind(line.title.as_deref())
    .bind(line.quantity)
    .bind(line.unit_price.amount)
    .bind(line.unit_price.currency.as_str())
    .fetch_one(&mut **tx)
    .await?)
}

/// Writes one thing that happened to a contract. Append-only and evidence: the
/// support conversation is answered from here.
async fn record(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    kind: &str,
    payload: Value,
) -> Result<()> {
    sqlx::query(
        "insert into subscription_event (id, scope, subscription_id, kind, payload, at)
         values ($1, $2, $3, $4, $5, $6)",
    )
    .bind(SubscriptionEventId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(kind)
    .bind(payload)
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

pub async fn get(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: SubscriptionId) -> Result<Subscription> {
    let found = sqlx::query_as::<_, Subscription>(&format!(
        "select {SUBSCRIPTION_COLUMNS} from subscription where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("subscription"))?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Subscription {
            id: Some(id.as_uuid()),
            customer: Some(found.customer_id.as_uuid()),
        },
    )?;

    Ok(found)
}

/// Every contract, newest last, optionally one customer's.
pub async fn list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer: Option<CustomerId>,
    paging: Paging,
) -> Result<Page<Subscription>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Subscription {
            id: None,
            customer: customer.map(CustomerId::as_uuid),
        },
    )?;

    let rows = sqlx::query_as::<_, Subscription>(&format!(
        "select {SUBSCRIPTION_COLUMNS} from subscription
         where scope = $1
           and ($2::uuid is null or customer_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(customer.map(CustomerId::as_uuid))
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

/// The contracts owed a renewal at `at`, oldest first.
///
/// This is the whole of tezgah's involvement with time: the host's cron asks
/// this and calls [`Renewals::renew`] on what comes back. A contract already
/// past due is included — dunning is a second attempt at the same cycle, and
/// leaving it out would mean the only retry was the job queue's.
pub async fn due(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    at: DateTime<Utc>,
    paging: Paging,
) -> Result<Page<Subscription>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Subscription {
            id: None,
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, Subscription>(&format!(
        "select {SUBSCRIPTION_COLUMNS} from subscription
         where scope = $1
           and status in ('active', 'past_due')
           and next_billing_at <= $2
           and ($3::timestamptz is null or (next_billing_at, id) > ($3, $4))
         order by next_billing_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(at)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.next_billing_at,
        id: row.id.as_uuid(),
    }))
}

/// What a contract is for, at the version it is on.
pub async fn lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
) -> Result<Vec<SubscriptionLine>> {
    let contract = get(tx, ctx, id).await?;

    Ok(sqlx::query_as::<_, SubscriptionLine>(&format!(
        "select {LINE_COLUMNS} from subscription_line
         where scope = $1 and subscription_id = $2 and version = $3
         order by created_at, id
         limit $4"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(contract.line_version)
    .bind(MAX_LINES)
    .fetch_all(&mut **tx)
    .await?)
}

/// Why it did what it did, oldest first.
pub async fn events(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    paging: Paging,
) -> Result<Page<SubscriptionEvent>> {
    get(tx, ctx, id).await?;

    let rows = sqlx::query_as::<_, SubscriptionEvent>(&format!(
        "select {EVENT_COLUMNS} from subscription_event
         where scope = $1 and subscription_id = $2
           and ($3::timestamptz is null or (at, id) > ($3, $4))
         order by at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.at,
        id: row.id.as_uuid(),
    }))
}

/// Ends a contract, now or when the period somebody has already paid for runs
/// out.
///
/// At period end nothing is refunded and nothing is taken away: the contract
/// simply does not renew, which is why it is a flag rather than a status.
pub async fn cancel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SubscriptionId,
    at_period_end: bool,
    reason: Option<&str>,
) -> Result<Subscription> {
    let contract = get(tx, ctx, id).await?;
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Subscription {
            id: Some(id.as_uuid()),
            customer: Some(contract.customer_id.as_uuid()),
        },
    )?;

    if !contract.is_live() {
        return Err(Error::conflict("that contract has already ended"));
    }

    let changed = sqlx::query_as::<_, Subscription>(&format!(
        "update subscription
         set cancel_at_period_end = true,
             status = case when $4 then 'cancelled' else status end,
             ended_at = case when $4 then $3 else ended_at end
         where scope = $1 and id = $2
         returning {SUBSCRIPTION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .bind(!at_period_end)
    .fetch_one(&mut **tx)
    .await?;

    record(
        tx,
        ctx,
        id,
        "cancelled",
        serde_json::json!({ "at_period_end": at_period_end, "reason": reason }),
    )
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "subscription",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "cancelled": true, "at_period_end": at_period_end }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "subscription.cancelled",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "at_period_end": at_period_end,
                "reason": reason,
            }),
        },
    )
    .await?;

    Ok(changed)
}

// ---------------------------------------------------------------------------
// Renewing
// ---------------------------------------------------------------------------

/// What the run is holding as it goes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Carried {
    subscription_id: Option<SubscriptionId>,
    customer_id: Option<CustomerId>,
    currency: Option<String>,
    cycle: Option<i32>,
    period_start: Option<DateTime<Utc>>,
    period_end: Option<DateTime<Utc>>,
    #[serde(default)]
    lines: Vec<PricedLine>,
    #[serde(default)]
    taxes: Vec<TaxOnLine>,
    #[serde(default)]
    reservations: Vec<ReservationId>,
    order_id: Option<OrderId>,
    payment_collection_id: Option<PaymentCollectionId>,
    payment_session_id: Option<PaymentSessionId>,
    payment_id: Option<PaymentId>,
}

impl Carried {
    fn of(input: &Value) -> std::result::Result<Self, Failure> {
        serde_json::from_value(input.clone())
            .map_err(|_| Failure::Final(Error::bug("a renewal step was handed something else")))
    }

    fn value(&self) -> std::result::Result<Value, Failure> {
        serde_json::to_value(self)
            .map_err(|_| Failure::Final(Error::bug("a renewal step could not keep its place")))
    }

    fn contract(&self) -> std::result::Result<SubscriptionId, Failure> {
        self.subscription_id
            .ok_or_else(|| Failure::Final(Error::invalid("a renewal needs a contract")))
    }

    fn money(&self, amount: Decimal) -> std::result::Result<Money, Failure> {
        let code = self
            .currency
            .as_deref()
            .ok_or_else(|| Failure::Final(Error::bug("a renewal lost its currency")))?;
        Ok(Money::new(
            amount,
            Currency::parse(code).map_err(Failure::Final)?,
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PricedLine {
    variant_id: VariantId,
    title: Option<String>,
    quantity: i32,
    unit_price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TaxOnLine {
    line: usize,
    rate: Decimal,
    code: String,
    name: String,
    treatment: String,
    jurisdiction_level: Option<String>,
    jurisdiction_code: Option<String>,
    jurisdiction_name: Option<String>,
    calculated_at: DateTime<Utc>,
    country_code: Option<String>,
    province_code: Option<String>,
    postal_code: Option<String>,
}

fn kept<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn recall<T: for<'de> Deserialize<'de> + Default>(kept: &Value) -> T {
    serde_json::from_value(kept.clone()).unwrap_or_default()
}

/// A deadlock or a serialisation failure is the database asking for the same
/// work again. Told apart by SQLSTATE, never by message.
fn wedged(err: Error) -> Failure {
    match err.sqlstate().as_deref() {
        Some("40001" | "40P01") => Failure::Retry(err),
        _ => Failure::Final(err),
    }
}

/// What came of asking a contract for another period.
#[derive(Debug, Clone)]
pub struct Renewed {
    pub run: workflow::Run,
    pub order_id: Option<OrderId>,
    /// The cycle this attempt was for, whether or not it billed.
    pub cycle: i32,
    /// The charge was refused and the contract is now `past_due`, with a
    /// dunning job queued or the contract cancelled for having run out of them.
    pub declined: bool,
    pub cancelled: bool,
}

/// How this shop renews: which provider charges the stored instrument, and
/// which location the stock comes off.
pub struct Renewals {
    provider: Arc<dyn RecurringProvider>,
    location_id: StockLocationId,
}

impl std::fmt::Debug for Renewals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Renewals")
            .field("provider", &self.provider.code())
            .field("location_id", &self.location_id)
            .finish()
    }
}

impl Renewals {
    pub fn new(provider: Arc<dyn RecurringProvider>, location_id: StockLocationId) -> Self {
        Renewals {
            provider,
            location_id,
        }
    }

    /// The renewal, in the order it has to happen in.
    ///
    /// The price is resolved rather than copied — leaving the contract's price
    /// with the provider is exactly what this exists not to do — and
    /// `advance_period` is last, so a failure anywhere leaves `next_billing_at`
    /// where it was and the next poll retries the same cycle under the same
    /// key: resume, not repeat.
    pub fn workflow(&self) -> Workflow {
        Workflow::new("subscription.renew")
            .locked_by(|input| {
                input
                    .get("subscription_id")
                    .and_then(Value::as_str)
                    .and_then(|text| Uuid::parse_str(text).ok())
            })
            .then(LoadContract)
            .then(ResolvePrice)
            .then(ReserveStock {
                location_id: self.location_id,
            })
            .then(CalculateTax)
            .then(CreateOrder)
            .then(ChargeStoredMethod {
                provider: Arc::clone(&self.provider),
            })
            .then(AdvancePeriod)
    }

    /// Bills one contract for the period it is owed.
    ///
    /// Takes the pool rather than a transaction: the engine opens one per step,
    /// so a step that finished stays finished when a later one does not. Runs
    /// as whatever actor the caller assembled, which for a host's cron is
    /// [`Actor::System`](crate::ports::Actor::System) — an authorizer that
    /// denies it stops every renewal in the shop.
    pub async fn renew(&self, pool: &PgPool, ctx: &Ctx<'_>, id: SubscriptionId) -> Result<Renewed> {
        let _: Permit = ctx.permit(
            Action::Write,
            Resource::Subscription {
                id: Some(id.as_uuid()),
                customer: None,
            },
        )?;

        let (cycle, attempts) = {
            let mut tx = scoped(pool, ctx).await?;
            let contract = get(&mut tx, ctx, id).await?;
            tx.commit().await?;
            (contract.cycle + 1, contract.dunning_attempts)
        };

        // The cycle is the key, so two schedulers firing at once are one run. A
        // dunning attempt takes a key of its own because a run that unwound is
        // settled, and starting the settled one again would hand back its
        // failure rather than trying the card: `subscription_order`'s unique
        // cycle is what still makes a second order impossible.
        let key = if attempts == 0 {
            format!("subscription:{id}:{cycle}")
        } else {
            format!("subscription:{id}:{cycle}:{attempts}")
        };

        let run = workflow::run(
            pool,
            ctx,
            &self.workflow(),
            &key,
            serde_json::json!({ "subscription_id": id, "cycle": cycle }),
        )
        .await?;

        let carried: Carried = recall(&run.output);
        let refused = run
            .failure
            .as_deref()
            .is_some_and(|why| why.starts_with(CHARGE_STEP));

        let mut declined = false;
        let mut cancelled = false;
        if refused {
            let outcome = decline(pool, ctx, id, cycle).await?;
            declined = true;
            cancelled = outcome;
        }

        Ok(Renewed {
            order_id: carried.order_id,
            cycle,
            declined,
            cancelled,
            run,
        })
    }
}

/// A transaction with this scope announced on it, which is the only kind the
/// policies admit.
async fn scoped(
    pool: &PgPool,
    ctx: &Ctx<'_>,
) -> Result<sqlx::Transaction<'static, sqlx::Postgres>> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(ctx.scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

/// The card said no: the contract goes `past_due` and the next attempt is
/// queued, or the contract is cancelled for having used every attempt it had.
///
/// One transaction, so the status and the job that retries it commit together —
/// a `past_due` with nothing coming back for it is a shop that has quietly
/// stopped charging somebody.
async fn decline(pool: &PgPool, ctx: &Ctx<'_>, id: SubscriptionId, cycle: i32) -> Result<bool> {
    let mut tx = scoped(pool, ctx).await?;

    let contract = sqlx::query_as::<_, Subscription>(&format!(
        "update subscription
         set status = 'past_due', dunning_attempts = dunning_attempts + 1
         where scope = $1 and id = $2 and status in ('active', 'past_due')
         returning {SUBSCRIPTION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut *tx)
    .await?;

    let Some(contract) = contract else {
        tx.rollback().await?;
        return Ok(false);
    };

    let plan = plan(&mut tx, ctx, contract.selling_plan_id).await?;

    record(
        &mut tx,
        ctx,
        id,
        "payment_failed",
        serde_json::json!({ "cycle": cycle, "attempt": contract.dunning_attempts }),
    )
    .await?;

    let exhausted = contract.dunning_attempts >= plan.dunning_max_attempts;

    if exhausted {
        sqlx::query(
            "update subscription set status = 'cancelled', ended_at = $3
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(ctx.now())
        .execute(&mut *tx)
        .await?;

        record(
            &mut tx,
            ctx,
            id,
            "dunning_exhausted",
            serde_json::json!({ "cycle": cycle, "attempts": contract.dunning_attempts }),
        )
        .await?;

        ctx.emit(
            &mut tx,
            Event {
                name: "subscription.cancelled",
                entity_id: id.as_uuid(),
                payload: serde_json::json!({
                    "reason": "dunning_exhausted",
                    "cycle": cycle,
                }),
            },
        )
        .await?;
    } else {
        let wait = chrono::Duration::try_hours(i64::from(
            plan.dunning_interval_hours
                .saturating_mul(contract.dunning_attempts),
        ))
        .ok_or_else(|| Error::invalid("that is not a length of time to wait"))?;

        ctx.enqueue(
            &mut tx,
            JobSpec {
                kind: DUNNING_JOB,
                payload: serde_json::json!({
                    "subscription": id,
                    "cycle": cycle,
                    "attempt": contract.dunning_attempts,
                }),
                run_after: Some(ctx.now() + wait),
            },
        )
        .await?;

        ctx.emit(
            &mut tx,
            Event {
                name: "subscription.payment_failed",
                entity_id: id.as_uuid(),
                payload: serde_json::json!({
                    "cycle": cycle,
                    "attempt": contract.dunning_attempts,
                }),
            },
        )
        .await?;
    }

    ctx.audit(
        &mut tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "subscription",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "declined": true,
                "attempt": contract.dunning_attempts,
                "cancelled": exhausted,
            }),
        },
    )
    .await?;

    tx.commit().await?;
    Ok(exhausted)
}

// ---------------------------------------------------------------------------
// 1. Is there a period to bill?
// ---------------------------------------------------------------------------

struct LoadContract;

#[async_trait]
impl Step for LoadContract {
    fn name(&self) -> &'static str {
        "load_contract"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let id: SubscriptionId =
            serde_json::from_value(input.get("subscription_id").cloned().unwrap_or(Value::Null))
                .map_err(|_| Failure::Final(Error::invalid("a renewal needs a contract")))?;

        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;

        if !contract.is_live() {
            return Err(Failure::Final(Error::conflict(
                "that contract is not one that renews",
            )));
        }
        if contract.paused_until.is_some_and(|until| until > ctx.now()) {
            return Err(Failure::Final(Error::conflict("that contract is paused")));
        }
        if contract.cancel_at_period_end {
            return Err(Failure::Final(Error::conflict(
                "that contract was asked to stop at the end of this period",
            )));
        }

        let plan = plan(tx, ctx, contract.selling_plan_id)
            .await
            .map_err(Failure::Final)?;

        let cycle = contract.cycle + 1;
        if plan.max_cycles.is_some_and(|most| cycle > most) {
            return Err(Failure::Final(Error::conflict(
                "that contract has had every cycle it was sold",
            )));
        }

        let already: Option<Uuid> = sqlx::query_scalar(
            "select order_id from subscription_order
             where scope = $1 and subscription_id = $2 and cycle = $3",
        )
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(cycle)
        .fetch_optional(&mut **tx)
        .await
        .map_err(|err| Failure::Retry(Error::from(err)))?;

        if already.is_some() {
            return Err(Failure::Final(Error::conflict(
                "that period has already been billed",
            )));
        }

        let period_start = contract.current_period_end;
        let period_end = advance(
            period_start,
            &plan.billing_interval_unit,
            plan.billing_interval_count,
        )
        .map_err(Failure::Final)?;

        let carried = Carried {
            subscription_id: Some(id),
            customer_id: Some(contract.customer_id),
            currency: Some(contract.currency_code.clone()),
            cycle: Some(cycle),
            period_start: Some(period_start),
            period_end: Some(period_end),
            ..Carried::default()
        };

        Ok(Outcome::new(carried.value()?, Value::Null))
    }
}

// ---------------------------------------------------------------------------
// 2. What does it cost today?
// ---------------------------------------------------------------------------

/// The version the contract was on before this step wrote a new one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PricedAt {
    subscription_id: Option<SubscriptionId>,
    was: Option<i32>,
    now: Option<i32>,
}

struct ResolvePrice;

#[async_trait]
impl Step for ResolvePrice {
    fn name(&self) -> &'static str {
        "resolve_price"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let id = carried.contract()?;

        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;
        let currency = contract.currency().map_err(Failure::Final)?;
        let plan = plan(tx, ctx, contract.selling_plan_id)
            .await
            .map_err(Failure::Final)?;
        let held = lines(tx, ctx, id).await.map_err(Failure::Final)?;
        if held.is_empty() {
            return Err(Failure::Final(Error::invalid(
                "that contract has nothing on it",
            )));
        }

        let exponent = store::exponent(tx, ctx, currency)
            .await
            .map_err(Failure::Final)?;

        let mut priced = Vec::with_capacity(held.len());
        let mut moved = false;

        for line in &held {
            let mut at = pricing::PriceContext::new(currency, line.quantity);
            if let Some(region) = contract.region_id {
                at = at.in_region(region);
            }
            if let Some(channel) = contract.sales_channel_id {
                at = at.through(channel.as_uuid());
            }

            let set = pricing::price_set_for_variant(tx, ctx, line.variant_id)
                .await
                .map_err(Failure::Final)?
                .ok_or_else(|| {
                    Failure::Final(Error::invalid("a line's variant is no longer for sale"))
                })?;

            let found = pricing::resolve(tx, ctx, set, &at)
                .await
                .map_err(Failure::Final)?
                .ok_or_else(|| {
                    Failure::Final(Error::invalid(
                        "a line's variant has no price in this currency",
                    ))
                })?;

            let unit = if plan.discounts_renewals() {
                discounted(found.calculated, &plan, exponent).map_err(Failure::Final)?
            } else {
                found.calculated
            };

            if unit.amount != line.unit_price {
                moved = true;
            }

            priced.push(PricedLine {
                variant_id: line.variant_id,
                title: line.title.clone(),
                quantity: line.quantity,
                unit_price: unit.amount,
            });
        }

        // A price that moved mid-contract writes a new version rather than
        // being edited in, so what the last cycle charged is still readable.
        let mut written = PricedAt {
            subscription_id: Some(id),
            was: Some(contract.line_version),
            now: Some(contract.line_version),
        };

        if moved {
            let version = contract.line_version + 1;
            for line in &priced {
                write_line(
                    tx,
                    ctx,
                    id,
                    version,
                    &NewLine {
                        variant_id: line.variant_id,
                        title: line.title.clone(),
                        quantity: line.quantity,
                        unit_price: Money::new(line.unit_price, currency),
                    },
                )
                .await
                .map_err(Failure::Final)?;
            }

            sqlx::query("update subscription set line_version = $3 where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(id.as_uuid())
                .bind(version)
                .execute(&mut **tx)
                .await
                .map_err(|err| Failure::Retry(Error::from(err)))?;

            record(
                tx,
                ctx,
                id,
                "price_changed",
                serde_json::json!({ "version": version, "cycle": carried.cycle }),
            )
            .await
            .map_err(Failure::Final)?;

            written.now = Some(version);
        }

        carried.lines = priced;

        Ok(Outcome::new(carried.value()?, kept(&written)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: PricedAt = recall(keep);
        let (Some(id), Some(was), Some(now)) = (undo.subscription_id, undo.was, undo.now) else {
            return Ok(());
        };
        if was == now {
            return Ok(());
        }

        sqlx::query(
            "delete from subscription_line
             where scope = $1 and subscription_id = $2 and version = $3",
        )
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(now)
        .execute(&mut **tx)
        .await?;

        sqlx::query("update subscription set line_version = $3 where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .bind(was)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }
}

/// The plan's own discount, taken off a resolved price rather than baked into
/// one: the price list is still what says what the thing costs.
fn discounted(price: Money, plan: &SellingPlan, exponent: u32) -> Result<Money> {
    let Some(value) = plan.discount_value else {
        return Ok(price);
    };

    let amount = match plan.discount_kind.as_deref() {
        Some("percentage") => {
            let left = Decimal::ONE_HUNDRED - value;
            (price.amount * left / Decimal::ONE_HUNDRED).round_dp(exponent)
        }
        Some("fixed") => {
            let code = plan
                .currency_code
                .as_deref()
                .ok_or_else(|| Error::bug("a fixed discount with no currency on it"))?;
            if Currency::parse(code)? != price.currency {
                return Err(Error::invalid(
                    "that plan's discount is in another currency",
                ));
            }
            price.amount - value
        }
        _ => return Ok(price),
    };

    Ok(Money::new(amount.max(Decimal::ZERO), price.currency))
}

// ---------------------------------------------------------------------------
// 3. Promise the stock
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HeldStock {
    reservations: Vec<ReservationId>,
}

struct ReserveStock {
    location_id: StockLocationId,
}

#[async_trait]
impl Step for ReserveStock {
    fn name(&self) -> &'static str {
        "reserve_stock"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;

        let mut wanted = Vec::new();
        for line in &carried.lines {
            let items = inventory::inventory_items_for_variant(tx, ctx, line.variant_id)
                .await
                .map_err(Failure::Final)?;

            for item in items {
                wanted.push((
                    item.inventory_item_id,
                    line.quantity * item.required_quantity,
                ));
            }
        }

        wanted.sort_by_key(|(item, _)| item.as_uuid());

        for (item, quantity) in wanted {
            let held =
                inventory::reserve(tx, ctx, item, self.location_id, quantity, None, false, None)
                    .await
                    .map_err(wedged)?;

            carried.reservations.push(held.id);
        }

        let undo = HeldStock {
            reservations: carried.reservations.clone(),
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: HeldStock = recall(keep);

        for id in undo.reservations {
            inventory::release(tx, ctx, id).await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 4. Today's rates, today's address
// ---------------------------------------------------------------------------

struct CalculateTax;

#[async_trait]
impl Step for CalculateTax {
    fn name(&self) -> &'static str {
        "calculate_tax"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let id = carried.contract()?;

        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;
        let Some(address) = taxable_address(tx, ctx, contract.shipping_address_id)
            .await
            .map_err(Failure::Final)?
        else {
            return Ok(Outcome::new(carried.value()?, Value::Null));
        };

        let mut taxable = Vec::with_capacity(carried.lines.len());
        for (at, line) in carried.lines.iter().enumerate() {
            let amount = carried.money(line.unit_price * Decimal::from(line.quantity))?;
            taxable.push(tax::TaxableLine {
                id: Uuid::from_u128(at as u128),
                amount,
                targets: vec![tax::TaxTarget {
                    reference: tax::TaxReference::Variant,
                    id: line.variant_id.as_uuid(),
                }],
                tax_code: None,
                address: None,
            });
        }

        let worked = tax::calculate(tx, ctx, &taxable, &address, None, false)
            .await
            .map_err(Failure::Final)?;

        carried.taxes = worked
            .into_iter()
            .map(|line| TaxOnLine {
                line: line.line_id.as_u128() as usize,
                rate: line.rate,
                code: line.code,
                name: line.name,
                treatment: line.treatment.as_str().to_string(),
                jurisdiction_level: line.jurisdiction.as_ref().map(|j| j.level.clone()),
                jurisdiction_code: line.jurisdiction.as_ref().map(|j| j.code.clone()),
                jurisdiction_name: line.jurisdiction.as_ref().and_then(|j| j.name.clone()),
                calculated_at: line.calculated_at,
                country_code: Some(line.address.country_code),
                province_code: line.address.province_code,
                postal_code: line.address.postal_code,
            })
            .collect();

        Ok(Outcome::new(carried.value()?, Value::Null))
    }
}

/// The contract's address, as the tax engine wants it.
async fn taxable_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Option<AddressId>,
) -> Result<Option<tax::TaxableAddress>> {
    let Some(id) = id else {
        return Ok(None);
    };

    let row: Option<(Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "select country_code, province, postal_code from customer_address
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.and_then(|(country, province, postal)| {
        country.map(|country_code| tax::TaxableAddress {
            country_code,
            province_code: province,
            postal_code: postal,
        })
    }))
}

// ---------------------------------------------------------------------------
// 5. Write the order for this period
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WrittenOrder {
    order_id: Option<OrderId>,
    subscription_order_id: Option<SubscriptionOrderId>,
}

struct CreateOrder;

#[async_trait]
impl Step for CreateOrder {
    fn name(&self) -> &'static str {
        "create_order"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let id = carried.contract()?;
        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;
        let currency = contract.currency().map_err(Failure::Final)?;

        let (Some(cycle), Some(period_start), Some(period_end)) =
            (carried.cycle, carried.period_start, carried.period_end)
        else {
            return Err(Failure::Final(Error::bug("a renewal lost its period")));
        };

        let total = carried.lines.iter().try_fold(
            Money::new(Decimal::ZERO, currency),
            |sum, line| -> std::result::Result<Money, Failure> {
                let line_total = carried.money(line.unit_price * Decimal::from(line.quantity))?;
                sum.plus(line_total).map_err(Failure::Final)
            },
        )?;

        let collection = payment::create_collection(
            tx,
            ctx,
            NewCollection {
                amount: total,
                cart_id: None,
                metadata: Some(serde_json::json!({ "subscription": id, "cycle": cycle })),
            },
        )
        .await
        .map_err(Failure::Final)?;

        let mut lines = Vec::with_capacity(carried.lines.len());
        for (at, line) in carried.lines.iter().enumerate() {
            let tax_lines = carried
                .taxes
                .iter()
                .filter(|found| found.line == at)
                .map(|found| NewTaxLine {
                    rate: found.rate,
                    code: found.code.clone(),
                    name: found.name.clone(),
                    provider_id: None,
                    description: None,
                    snapshot: TaxSnapshot {
                        treatment: Some(found.treatment.clone()),
                        jurisdiction_level: found.jurisdiction_level.clone(),
                        jurisdiction_code: found.jurisdiction_code.clone(),
                        jurisdiction_name: found.jurisdiction_name.clone(),
                        calculated_at: Some(found.calculated_at),
                        address_country_code: found.country_code.clone(),
                        address_province_code: found.province_code.clone(),
                        address_postal_code: found.postal_code.clone(),
                        ..TaxSnapshot::default()
                    },
                })
                .collect();

            let title = line.title.clone().unwrap_or_else(|| "Renewal".to_string());

            let mut ordered =
                NewOrderLine::of(title, line.quantity, Money::new(line.unit_price, currency));
            ordered.variant_id = Some(line.variant_id);
            ordered.tax_lines = tax_lines;
            lines.push(ordered);
        }

        let new = NewOrder {
            region_id: contract.region_id,
            sales_channel_id: contract.sales_channel_id,
            customer_id: Some(contract.customer_id),
            currency_code: currency,
            payment_collection_id: Some(collection.id),
            subscription_id: Some(id),
            shipping_address: copy_address(tx, ctx, contract.shipping_address_id)
                .await
                .map_err(Failure::Final)?,
            billing_address: copy_address(tx, ctx, contract.billing_address_id)
                .await
                .map_err(Failure::Final)?,
            lines,
            metadata: Some(serde_json::json!({ "cycle": cycle })),
            ..NewOrder::of(currency)
        };

        let placed = order::create(tx, ctx, new).await.map_err(Failure::Final)?;

        let join = SubscriptionOrderId::new();
        sqlx::query(
            "insert into subscription_order
                 (id, scope, subscription_id, order_id, cycle, period_start, period_end, kind)
             values ($1, $2, $3, $4, $5, $6, $7, 'renewal')",
        )
        .bind(join.as_uuid())
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(placed.id.as_uuid())
        .bind(cycle)
        .bind(period_start)
        .bind(period_end)
        .execute(&mut **tx)
        .await
        .map_err(|err| {
            let err = Error::from(err);
            match err.sqlstate().as_deref() {
                Some("23505") => Failure::Final(Error::conflict(
                    "that period has already been billed by somebody else",
                )),
                _ => wedged(err),
            }
        })?;

        carried.order_id = Some(placed.id);
        carried.payment_collection_id = Some(collection.id);

        let undo = WrittenOrder {
            order_id: Some(placed.id),
            subscription_order_id: Some(join),
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: WrittenOrder = recall(keep);
        let Some(order_id) = undo.order_id else {
            return Ok(());
        };

        sqlx::query("delete from subscription_order where scope = $1 and order_id = $2")
            .bind(ctx.scope.0)
            .bind(order_id.as_uuid())
            .execute(&mut **tx)
            .await?;

        erase_order(tx, ctx, order_id).await
    }
}

/// Copies the contract's address onto the order rather than pointing at it, the
/// way checkout does: the customer editing theirs next year must not move what
/// an order already sent said.
async fn copy_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Option<AddressId>,
) -> Result<Option<OrderAddress>> {
    let Some(id) = id else {
        return Ok(None);
    };

    #[derive(FromRow)]
    struct Row {
        company: Option<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        address_1: Option<String>,
        address_2: Option<String>,
        city: Option<String>,
        province: Option<String>,
        postal_code: Option<String>,
        country_code: Option<String>,
        phone: Option<String>,
    }

    let row = sqlx::query_as::<_, Row>(
        "select company, first_name, last_name, address_1, address_2, city, province,
                postal_code, country_code, phone
         from customer_address
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.map(|row| OrderAddress {
        company: row.company,
        first_name: row.first_name,
        last_name: row.last_name,
        address_1: row.address_1,
        address_2: row.address_2,
        city: row.city,
        province: row.province,
        postal_code: row.postal_code,
        country_code: row.country_code,
        phone: row.phone,
    }))
}

/// Takes back an order this run wrote moments ago, children first. Every key
/// below is `on delete restrict`, so a delete that hits something a later step
/// added fails loudly rather than tearing a live order apart.
async fn erase_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<()> {
    for (table, parent, key) in [
        (
            "order_line_item_adjustment",
            "order_line_item",
            "order_line_item_id",
        ),
        (
            "order_line_item_tax_line",
            "order_line_item",
            "order_line_item_id",
        ),
    ] {
        sqlx::query(&format!(
            "delete from {table}
             where scope = $1
               and {key} in (select id from {parent} where scope = $1 and order_id = $2)"
        ))
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    for table in [
        "order_status_history",
        "order_summary",
        "order_item",
        "order_transaction",
        "order_line_item",
    ] {
        sqlx::query(&format!(
            "delete from {table} where scope = $1 and order_id = $2"
        ))
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    let addresses: Vec<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"delete from "order" where scope = $1 and id = $2
           returning shipping_address_id, billing_address_id"#,
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for (shipping, billing) in addresses {
        for address in [shipping, billing].into_iter().flatten() {
            sqlx::query("delete from order_address where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(address)
                .execute(&mut **tx)
                .await?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// 6. Charge the instrument the provider is holding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HeldMoney {
    payment_id: Option<PaymentId>,
    order_id: Option<OrderId>,
}

struct ChargeStoredMethod {
    provider: Arc<dyn RecurringProvider>,
}

#[async_trait]
impl Step for ChargeStoredMethod {
    fn name(&self) -> &'static str {
        CHARGE_STEP
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let id = carried.contract()?;
        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;

        let collection_id = carried
            .payment_collection_id
            .ok_or_else(|| Failure::Final(Error::bug("a renewal lost its payment collection")))?;
        let order_id = carried
            .order_id
            .ok_or_else(|| Failure::Final(Error::bug("a renewal lost its order")))?;

        let holder_id = contract.account_holder_id.ok_or_else(|| {
            Failure::Final(Error::invalid(
                "that contract names no account with the provider",
            ))
        })?;
        let method = contract.payment_method_reference.clone().ok_or_else(|| {
            Failure::Final(Error::invalid("that contract holds no stored instrument"))
        })?;

        let holder = payment::account_holder_by_id(tx, ctx, holder_id)
            .await
            .map_err(Failure::Final)?
            .ok_or_else(|| Failure::Final(Error::not_found("account holder")))?;

        let collection = payment::collection(tx, ctx, collection_id)
            .await
            .map_err(Failure::Final)?;
        let owed = collection.due_total().map_err(Failure::Final)?;

        if owed.amount <= Decimal::ZERO {
            return Ok(Outcome::new(carried.value()?, Value::Null));
        }

        let session = payment::create_session(
            tx,
            ctx,
            NewSession {
                collection_id,
                provider_code: self.provider.code().to_string(),
                amount: owed,
                context: Some(serde_json::json!({ "subscription": id, "order_id": order_id })),
                installment_count: None,
            },
        )
        .await
        .map_err(Failure::Final)?;

        let answer = self
            .provider
            .authorize_stored(StoredChargeRequest {
                session_id: session.id,
                collection_id,
                amount: owed,
                account_holder: holder.external_id.clone(),
                payment_method_reference: method,
                mandate_reference: contract.mandate_reference.clone(),
                context: serde_json::json!({ "subscription": id, "cycle": carried.cycle }),
            })
            .await
            .map_err(Failure::Retry)?;

        // Off-session, a second factor is a refusal: there is nobody in a
        // browser to send anywhere, and a hold nobody will ever complete is
        // worse than a decline the shop can dun for.
        if answer.status == AuthorizationStatus::RequiresMore {
            return Err(Failure::Final(Error::provider(
                "payment",
                "the provider wants the shopper, and there is no shopper",
            )));
        }

        let authorized = payment::authorize(tx, ctx, session.id, answer)
            .await
            .map_err(Failure::Final)?;

        carried.payment_session_id = Some(session.id);

        match authorized {
            Authorized::Payment(paid) => {
                order::record_authorization(
                    tx,
                    ctx,
                    paid.payment_collection_id,
                    paid.id,
                    Money::new(paid.amount, owed.currency),
                )
                .await
                .map_err(Failure::Final)?;

                carried.payment_id = Some(paid.id);

                let undo = HeldMoney {
                    payment_id: Some(paid.id),
                    order_id: Some(order_id),
                };

                Ok(Outcome::new(carried.value()?, kept(&undo)))
            }
            Authorized::RequiresMore(_) | Authorized::Failed(_) => Err(Failure::Final(
                Error::provider("payment", "the provider would not hold the money"),
            )),
        }
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: HeldMoney = recall(keep);
        let Some(payment_id) = undo.payment_id else {
            return Ok(());
        };

        payment::cancel(tx, ctx, payment_id).await?;

        if let Some(order_id) = undo.order_id {
            sqlx::query(
                "delete from order_transaction
                 where scope = $1 and order_id = $2 and reference = 'payment'
                   and reference_id = $3",
            )
            .bind(ctx.scope.0)
            .bind(order_id.as_uuid())
            .bind(payment_id.as_uuid())
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 7. Only now does the clock move
// ---------------------------------------------------------------------------

struct AdvancePeriod;

#[async_trait]
impl Step for AdvancePeriod {
    fn name(&self) -> &'static str {
        "advance_period"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let carried = Carried::of(input)?;
        let id = carried.contract()?;

        let (Some(cycle), Some(period_start), Some(period_end)) =
            (carried.cycle, carried.period_start, carried.period_end)
        else {
            return Err(Failure::Final(Error::bug("a renewal lost its period")));
        };

        let contract = get(tx, ctx, id).await.map_err(Failure::Final)?;
        let plan = plan(tx, ctx, contract.selling_plan_id)
            .await
            .map_err(Failure::Final)?;

        let spent = plan.max_cycles.is_some_and(|most| cycle >= most);
        let status = if spent { "expired" } else { "active" };

        sqlx::query(
            "update subscription
             set current_period_start = $3, current_period_end = $4, next_billing_at = $4,
                 cycle = $5, status = $6, dunning_attempts = 0,
                 ended_at = case when $7 then $8 else ended_at end
             where scope = $1 and id = $2",
        )
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .bind(period_start)
        .bind(period_end)
        .bind(cycle)
        .bind(status)
        .bind(spent)
        .bind(ctx.now())
        .execute(&mut **tx)
        .await
        .map_err(|err| Failure::Retry(Error::from(err)))?;

        record(
            tx,
            ctx,
            id,
            "renewed",
            serde_json::json!({
                "cycle": cycle,
                "order": carried.order_id,
                "period_end": period_end,
            }),
        )
        .await
        .map_err(Failure::Final)?;

        if spent {
            record(
                tx,
                ctx,
                id,
                "expired",
                serde_json::json!({ "cycle": cycle }),
            )
            .await
            .map_err(Failure::Final)?;
        }

        ctx.audit(
            tx,
            AuditEntry {
                actor: ctx.actor.clone(),
                action: Action::Settle,
                entity: "subscription",
                entity_id: id.as_uuid(),
                summary: serde_json::json!({ "cycle": cycle, "order": carried.order_id }),
            },
        )
        .await
        .map_err(Failure::Final)?;

        ctx.emit(
            tx,
            Event {
                name: "subscription.renewed",
                entity_id: id.as_uuid(),
                payload: serde_json::json!({
                    "cycle": cycle,
                    "order": carried.order_id,
                    "next_billing_at": period_end,
                }),
            },
        )
        .await
        .map_err(Failure::Final)?;

        Ok(Outcome::new(carried.value()?, Value::Null))
    }
}
