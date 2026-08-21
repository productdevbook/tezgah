//! What comes off a cart, who may have it, and how often.
//!
//! A promotion carries one application method: what it takes off, what it takes
//! it off, and whether the amount is meant per line or shared between them. A
//! shared amount is split with [`crate::money::allocate`], so the parts add back
//! up to the discount that was granted rather than to something a rounding step
//! invented.
//!
//! Usage is a claim rather than a count. [`claim`] moves the counter with an
//! update the database refuses when it would pass the limit, so two checkouts
//! racing for the last one cannot both win, and no read comes first.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    AdjustmentId, CampaignId, CartId, CustomerId, LineItemId, PromotionId, ShippingMethodId,
};
use crate::money::{Currency, Money, allocate};
use crate::page::{Cursor, Order, Page, Paging, Search};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};
use crate::store;

/// Most rules one set of one promotion may be read back with.
const MAX_RULES: i64 = 500;
const MAX_SHIPPING_METHODS: i64 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionKind {
    Standard,
    BuyGet,
}

impl PromotionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PromotionKind::Standard => "standard",
            PromotionKind::BuyGet => "buyget",
        }
    }
}

/// The predicates `promotions` pages over and counts with, in one place so
/// the two cannot drift — the mistake this codebase has made with one fact
/// answered twice.
macro_rules! promotion_filter {
    () => {
        "
         where scope = $1
           and deleted_at is null
           and ($2::text is null or status = $2)
           and ($3::text is null or type = $3)
           and ($4::uuid is null or campaign_id = $4)
           and ($5::boolean is null or is_automatic = $5)
           and ($6::text is null or code ilike $6)
"
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Draft,
    Active,
    Inactive,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Active => "active",
            Status::Inactive => "inactive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MethodKind {
    Fixed,
    Percentage,
}

impl MethodKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MethodKind::Fixed => "fixed",
            MethodKind::Percentage => "percentage",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetKind {
    Order,
    Items,
    ShippingMethods,
}

impl TargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Order => "order",
            TargetKind::Items => "items",
            TargetKind::ShippingMethods => "shipping_methods",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Allocation {
    /// The value is taken off each line it lands on.
    Each,
    /// The value is one amount, shared between the lines it lands on.
    Across,
    /// The value lands on the cheapest units of the target, up to
    /// `max_quantity` units total — the same cheapest-first quota `buy_get`
    /// always uses, reached here by a plain promotion. Not valid against
    /// `target_type: order`, which has no units to sort by price.
    Once,
}

impl Allocation {
    pub fn as_str(self) -> &'static str {
        match self {
            Allocation::Each => "each",
            Allocation::Across => "across",
            Allocation::Once => "once",
        }
    }
}

/// Which set of conditions a rule belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleKind {
    /// Whether the cart may have the promotion at all.
    Eligibility,
    /// Which lines the discount lands on.
    Target,
    /// Which lines have to be in the cart first.
    Buy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Operator {
    Eq,
    Ne,
    In,
    Gt,
    Lt,
    Gte,
    Lte,
}

impl Operator {
    pub fn as_str(self) -> &'static str {
        match self {
            Operator::Eq => "eq",
            Operator::Ne => "ne",
            Operator::In => "in",
            Operator::Gt => "gt",
            Operator::Lt => "lt",
            Operator::Gte => "gte",
            Operator::Lte => "lte",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "eq" => Ok(Operator::Eq),
            "ne" => Ok(Operator::Ne),
            "in" => Ok(Operator::In),
            "gt" => Ok(Operator::Gt),
            "lt" => Ok(Operator::Lt),
            "gte" => Ok(Operator::Gte),
            "lte" => Ok(Operator::Lte),
            other => Err(Error::invalid(format!("{other:?} is not an operator"))),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Campaign {
    pub id: CampaignId,
    pub identifier: String,
    pub name: String,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewCampaign {
    pub identifier: String,
    pub name: String,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BudgetKind {
    /// A limit on money given away, in aggregate across every customer.
    Spend,
    /// A limit on how many times the campaign is claimed, in aggregate.
    Usage,
    /// A limit on how many times the campaign is claimed by one customer (or
    /// whatever `attribute` names), across every promotion it runs.
    UseByAttribute,
    /// A limit on money given away to one customer (or whatever `attribute`
    /// names), across every promotion the campaign runs.
    SpendByAttribute,
}

impl BudgetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetKind::Spend => "spend",
            BudgetKind::Usage => "usage",
            BudgetKind::UseByAttribute => "use_by_attribute",
            BudgetKind::SpendByAttribute => "spend_by_attribute",
        }
    }

    fn is_by_attribute(self) -> bool {
        matches!(
            self,
            BudgetKind::UseByAttribute | BudgetKind::SpendByAttribute
        )
    }

    fn needs_currency(self) -> bool {
        matches!(self, BudgetKind::Spend | BudgetKind::SpendByAttribute)
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CampaignBudget {
    pub id: Uuid,
    pub campaign_id: CampaignId,
    #[sqlx(rename = "type")]
    pub kind: String,
    #[sqlx(rename = "limit")]
    pub cap: Option<Decimal>,
    pub used: Decimal,
    pub currency_code: Option<String>,
    pub attribute: Option<String>,
}

/// One claim's worth of a by-attribute budget: what one customer (or whatever
/// `attribute` names) has used of it, across every promotion the campaign
/// runs.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CampaignBudgetUsage {
    pub id: Uuid,
    pub campaign_budget_id: Uuid,
    pub attribute_value: String,
    pub used: Decimal,
    #[sqlx(rename = "limit")]
    pub cap: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct NewCampaignBudget {
    pub campaign_id: CampaignId,
    pub kind: BudgetKind,
    pub cap: Option<Decimal>,
    pub currency_code: Option<Currency>,
    /// Which field a claim's context is resolved by, for `UseByAttribute` and
    /// `SpendByAttribute` — `customer_id` today, since that is what a claim's
    /// context carries.
    pub attribute: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Promotion {
    pub id: PromotionId,
    pub campaign_id: Option<CampaignId>,
    pub code: String,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub status: String,
    pub is_automatic: bool,
    pub usage_limit: Option<i32>,
    pub used: i32,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewPromotion {
    pub code: String,
    pub kind: PromotionKind,
    pub status: Status,
    pub is_automatic: bool,
    pub campaign_id: Option<CampaignId>,
    pub usage_limit: Option<i32>,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ApplicationMethod {
    pub id: Uuid,
    pub promotion_id: PromotionId,
    #[sqlx(rename = "type")]
    pub kind: String,
    pub target_type: String,
    pub allocation: Option<String>,
    pub value: Option<Decimal>,
    pub currency_code: Option<String>,
    pub max_quantity: Option<i32>,
    pub apply_to_quantity: Option<i32>,
    pub buy_rules_min_quantity: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct NewApplicationMethod {
    pub promotion_id: PromotionId,
    pub kind: MethodKind,
    pub target_type: TargetKind,
    pub allocation: Option<Allocation>,
    pub value: Decimal,
    pub currency_code: Option<Currency>,
    pub max_quantity: Option<i32>,
    /// How many units of the target the discount reaches, for a buy-X-get-Y.
    pub apply_to_quantity: Option<i32>,
    /// How many units of the buy side have to be there first.
    pub buy_rules_min_quantity: Option<i32>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Rule {
    pub id: Uuid,
    pub attribute: String,
    pub operator: String,
    pub allowed_values: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewRule {
    pub attribute: String,
    pub operator: Operator,
    pub allowed_values: Vec<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdjustmentTarget {
    LineItem(LineItemId),
    ShippingMethod(ShippingMethodId),
}

/// What one promotion took off one line.
#[derive(Debug, Clone)]
pub struct Adjustment {
    pub id: AdjustmentId,
    pub promotion_id: PromotionId,
    pub code: String,
    pub target: AdjustmentTarget,
    pub amount: Money,
    pub description: Option<String>,
}

pub async fn create_campaign(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewCampaign) -> Result<Campaign> {
    let _: Permit = ctx.permit(Action::Write, Resource::Promotion { id: None })?;

    if new.identifier.trim().is_empty() || new.name.trim().is_empty() {
        return Err(Error::invalid("a campaign needs an identifier and a name"));
    }

    let id = CampaignId::new();
    let campaign = sqlx::query_as::<_, Campaign>(
        "insert into campaign
             (id, scope, identifier, name, description, starts_at, ends_at, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, identifier, name, description, starts_at, ends_at, metadata, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.identifier.trim())
    .bind(new.name.trim())
    .bind(new.description.as_deref())
    .bind(new.starts_at)
    .bind(new.ends_at)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "campaign",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "identifier": campaign.identifier }),
        },
    )
    .await?;

    Ok(campaign)
}

pub async fn campaigns(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<Campaign>> {
    let _: Permit = ctx.permit(Action::View, Resource::Promotion { id: None })?;

    let rows = sqlx::query_as::<_, Campaign>(
        "select id, identifier, name, description, starts_at, ends_at, metadata, created_at
         from campaign
         where scope = $1
           and ($2::timestamptz is null or (created_at, id) > ($2, $3))
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

/// One budget per campaign, so setting it twice is an edit rather than a
/// second ceiling nothing reads.
pub async fn set_campaign_budget(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewCampaignBudget,
) -> Result<CampaignBudget> {
    let _: Permit = ctx.permit(Action::Write, Resource::Promotion { id: None })?;

    if new.kind.needs_currency() && new.currency_code.is_none() {
        return Err(Error::invalid("a spend budget needs a currency"));
    }
    if new.kind.is_by_attribute() && new.attribute.as_deref().is_none_or(str::is_empty) {
        return Err(Error::invalid("a budget by attribute needs to name it"));
    }

    let budget = sqlx::query_as::<_, CampaignBudget>(
        r#"insert into campaign_budget
               (id, scope, campaign_id, type, "limit", currency_code, attribute)
           values ($1, $2, $3, $4, $5, $6, $7)
           on conflict (scope, campaign_id) do update
             set type = excluded.type,
                 "limit" = excluded."limit",
                 currency_code = excluded.currency_code,
                 attribute = excluded.attribute
           returning id, campaign_id, type, "limit", used, currency_code, attribute"#,
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.campaign_id.as_uuid())
    .bind(new.kind.as_str())
    .bind(new.cap)
    .bind(new.currency_code.map(|code| code.as_str().to_string()))
    .bind(new.attribute.as_deref().map(str::trim))
    .fetch_one(&mut **tx)
    .await?;

    Ok(budget)
}

/// What every attribute value a per-attribute budget has been claimed against
/// has used of it — the customers (or whatever `attribute` names) counted
/// toward a "$50 per customer across the campaign" cap, and how close each is
/// to it. Configuration-adjacent rather than a customer's own data, so it is
/// capped rather than paged, the way a promotion's own rule sets are.
pub async fn campaign_budget_usage(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
) -> Result<Vec<CampaignBudgetUsage>> {
    let _: Permit = ctx.permit(Action::View, Resource::Promotion { id: None })?;

    let rows = sqlx::query_as::<_, CampaignBudgetUsage>(
        r#"select u.id, u.campaign_budget_id, u.attribute_value, u.used, u."limit"
           from campaign_budget_usage u
           join campaign_budget b on b.id = u.campaign_budget_id and b.scope = u.scope
           where u.scope = $1 and b.campaign_id = $2
           order by u.attribute_value
           limit $3"#,
    )
    .bind(ctx.scope.0)
    .bind(campaign_id.as_uuid())
    .bind(MAX_RULES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn campaign(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CampaignId) -> Result<Campaign> {
    let _: Permit = ctx.permit(Action::View, Resource::Promotion { id: None })?;

    sqlx::query_as::<_, Campaign>(
        "select id, identifier, name, description, starts_at, ends_at, metadata, created_at
         from campaign
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("campaign"))
}

/// A field left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct CampaignPatch {
    pub identifier: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub starts_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ends_at: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_campaign(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CampaignId,
    patch: CampaignPatch,
) -> Result<Campaign> {
    let _: Permit = ctx.permit(Action::Write, Resource::Promotion { id: None })?;

    if patch
        .identifier
        .as_ref()
        .is_some_and(|text| text.trim().is_empty())
        || patch
            .name
            .as_ref()
            .is_some_and(|text| text.trim().is_empty())
    {
        return Err(Error::invalid("a campaign needs an identifier and a name"));
    }

    let campaign = sqlx::query_as::<_, Campaign>(
        "update campaign set
             identifier = coalesce($3::text, identifier),
             name = coalesce($4::text, name),
             description = coalesce($5::text, description),
             starts_at = coalesce($6::timestamptz, starts_at),
             ends_at = coalesce($7::timestamptz, ends_at),
             metadata = coalesce($8::jsonb, metadata)
         where scope = $1 and id = $2
         returning id, identifier, name, description, starts_at, ends_at, metadata, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.identifier.as_deref().map(str::trim))
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.description.as_deref())
    .bind(patch.starts_at)
    .bind(patch.ends_at)
    .bind(patch.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("campaign"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "campaign",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "identifier": campaign.identifier }),
        },
    )
    .await?;

    Ok(campaign)
}

/// Puts a promotion under a campaign, which is what makes the campaign's budget
/// govern anything at all: a campaign with nothing on it spends nothing.
pub async fn attach_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    promotion_id: PromotionId,
) -> Result<Promotion> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    // The foreign key does not carry the scope, so the campaign is looked for
    // in this one before it is pointed at.
    let known: Option<Uuid> =
        sqlx::query_scalar("select id from campaign where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(campaign_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;
    if known.is_none() {
        return Err(Error::not_found("campaign"));
    }

    set_campaign(tx, ctx, promotion_id, Some(campaign_id)).await
}

pub async fn detach_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    promotion_id: PromotionId,
) -> Result<Promotion> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    let current = sqlx::query_scalar::<_, Option<Uuid>>(
        "select campaign_id from promotion where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(promotion_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("promotion"))?;

    if current != Some(campaign_id.as_uuid()) {
        return Err(Error::conflict("that promotion is not on that campaign"));
    }

    set_campaign(tx, ctx, promotion_id, None).await
}

async fn set_campaign(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    campaign_id: Option<CampaignId>,
) -> Result<Promotion> {
    let promotion = sqlx::query_as::<_, Promotion>(
        "update promotion set campaign_id = $3
         where scope = $1 and id = $2
         returning id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                   customer_usage_limit, metadata, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(campaign_id.map(CampaignId::as_uuid))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("promotion"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "promotion",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "campaign": campaign_id.map(|c| c.to_string()) }),
        },
    )
    .await?;

    Ok(promotion)
}

/// A field left `None` is left alone. A limit already reached is not lowered
/// out of trouble: `used` is what was given away and stays where it is.
#[derive(Debug, Clone, Default)]
pub struct PromotionPatch {
    pub code: Option<String>,
    pub is_automatic: Option<bool>,
    pub usage_limit: Option<i32>,
    pub customer_usage_limit: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn update_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    patch: PromotionPatch,
) -> Result<Promotion> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(id.as_uuid()),
        },
    )?;

    if patch
        .code
        .as_ref()
        .is_some_and(|code| code.trim().is_empty())
    {
        return Err(Error::invalid("a promotion needs a code"));
    }
    if patch.usage_limit.is_some_and(|limit| limit < 0)
        || patch.customer_usage_limit.is_some_and(|limit| limit < 0)
    {
        return Err(Error::invalid("a usage limit is not negative"));
    }

    let promotion = sqlx::query_as::<_, Promotion>(
        "update promotion set
             code = coalesce($3::text, code),
             is_automatic = coalesce($4::boolean, is_automatic),
             usage_limit = coalesce($5::integer, usage_limit),
             customer_usage_limit = coalesce($6::integer, customer_usage_limit),
             metadata = coalesce($7::jsonb, metadata)
         where scope = $1 and id = $2
         returning id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                   customer_usage_limit, metadata, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.code.as_deref().map(str::trim))
    .bind(patch.is_automatic)
    .bind(patch.usage_limit)
    .bind(patch.customer_usage_limit)
    .bind(patch.metadata)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("promotion"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "promotion",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "code": promotion.code }),
        },
    )
    .await?;

    Ok(promotion)
}

pub async fn create_promotion(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewPromotion,
) -> Result<Promotion> {
    let id = PromotionId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(id.as_uuid()),
        },
    )?;

    if new.code.trim().is_empty() {
        return Err(Error::invalid("a promotion needs a code"));
    }

    let promotion = sqlx::query_as::<_, Promotion>(
        "insert into promotion
             (id, scope, campaign_id, code, type, status, is_automatic, usage_limit,
              customer_usage_limit, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                   customer_usage_limit, metadata, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.campaign_id.map(CampaignId::as_uuid))
    .bind(new.code.trim())
    .bind(new.kind.as_str())
    .bind(new.status.as_str())
    .bind(new.is_automatic)
    .bind(new.usage_limit)
    .bind(new.customer_usage_limit)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "promotion",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "code": promotion.code }),
        },
    )
    .await?;

    Ok(promotion)
}

pub async fn promotion(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PromotionId) -> Result<Promotion> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Promotion {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, Promotion>(
        "select id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                customer_usage_limit, metadata, created_at
         from promotion
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("promotion"))
}

/// What a back office can ask a list of promotions.
///
/// Every field is `None` by default, so `PromotionFilter::default()` is the
/// list this function answered before it took one.
#[derive(Debug, Clone, Default)]
pub struct PromotionFilter {
    pub status: Option<Status>,
    pub kind: Option<PromotionKind>,
    pub campaign: Option<CampaignId>,
    /// Automatic promotions apply themselves; the rest wait for a code. A
    /// shop looking for "the discount somebody typed" wants one or the other.
    pub automatic: Option<bool>,
    /// Matched against the code, which is the only thing a person recognises
    /// a promotion by — a name it does not have and a description it does not
    /// carry.
    pub search: Option<Search>,
    pub order: Order,
}

pub async fn promotions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    filter: PromotionFilter,
    paging: Paging,
) -> Result<Page<Promotion>> {
    let _: Permit = ctx.permit(Action::View, Resource::Promotion { id: None })?;

    let beyond = filter.order.beyond();
    let direction = filter.order.direction();

    let rows = sqlx::query_as::<_, Promotion>(&format!(
        concat!(
            "select id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                customer_usage_limit, metadata, created_at
         from promotion",
            promotion_filter!(),
            "           and ($7::timestamptz is null or (created_at, id) {beyond} ($7, $8))
         order by created_at {direction}, id {direction}
         limit $9"
        ),
        beyond = beyond,
        direction = direction,
    ))
    .bind(ctx.scope.0)
    .bind(filter.status.map(Status::as_str))
    .bind(filter.kind.map(PromotionKind::as_str))
    .bind(filter.campaign.map(CampaignId::as_uuid))
    .bind(filter.automatic)
    .bind(filter.search.as_ref().map(Search::pattern))
    .bind(paging.after.as_ref().and_then(Cursor::timestamp))
    .bind(paging.after.as_ref().map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    let total = if paging.counts() {
        let (count,): (i64,) = sqlx::query_as(concat!(
            "select count(*) from promotion",
            promotion_filter!()
        ))
        .bind(ctx.scope.0)
        .bind(filter.status.map(Status::as_str))
        .bind(filter.kind.map(PromotionKind::as_str))
        .bind(filter.campaign.map(CampaignId::as_uuid))
        .bind(filter.automatic)
        .bind(filter.search.as_ref().map(Search::pattern))
        .fetch_one(&mut **tx)
        .await?;
        Some(count)
    } else {
        None
    };

    let page = Page::build(rows, paging, |row| {
        Cursor::at(row.created_at, row.id.as_uuid())
    });

    Ok(match total {
        Some(total) => page.counting(total),
        None => page,
    })
}

pub async fn set_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PromotionId,
    status: Status,
) -> Result<Promotion> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, Promotion>(
        "update promotion
         set status = $3
         where scope = $1 and id = $2 and deleted_at is null
         returning id, campaign_id, code, type, status, is_automatic, usage_limit, used,
                   customer_usage_limit, metadata, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(status.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("promotion"))
}

/// Withdrawn rather than erased: the discounts it already granted happened.
pub async fn delete_promotion(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PromotionId) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Promotion {
            id: Some(id.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "update promotion set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("promotion"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "promotion",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

pub async fn set_application_method(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewApplicationMethod,
) -> Result<ApplicationMethod> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(new.promotion_id.as_uuid()),
        },
    )?;

    if new.kind == MethodKind::Percentage && new.value > Decimal::from(100) {
        return Err(Error::invalid("a percentage above a hundred"));
    }
    if new.kind == MethodKind::Fixed && new.currency_code.is_none() {
        return Err(Error::invalid("a fixed discount needs a currency"));
    }
    if new.allocation == Some(Allocation::Once) && new.target_type == TargetKind::Order {
        return Err(Error::invalid(
            "an allocation of once needs items or shipping methods, not the whole order",
        ));
    }

    let method = sqlx::query_as::<_, ApplicationMethod>(
        "insert into application_method
             (id, scope, promotion_id, type, target_type, allocation, value, currency_code,
              max_quantity, apply_to_quantity, buy_rules_min_quantity)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         on conflict (scope, promotion_id) do update
           set type = excluded.type,
               target_type = excluded.target_type,
               allocation = excluded.allocation,
               value = excluded.value,
               currency_code = excluded.currency_code,
               max_quantity = excluded.max_quantity,
               apply_to_quantity = excluded.apply_to_quantity,
               buy_rules_min_quantity = excluded.buy_rules_min_quantity
         returning id, promotion_id, type, target_type, allocation, value, currency_code,
                   max_quantity, apply_to_quantity, buy_rules_min_quantity",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.promotion_id.as_uuid())
    .bind(new.kind.as_str())
    .bind(new.target_type.as_str())
    .bind(new.allocation.map(Allocation::as_str))
    .bind(new.value)
    .bind(new.currency_code.map(|code| code.as_str().to_string()))
    .bind(new.max_quantity)
    .bind(new.apply_to_quantity)
    .bind(new.buy_rules_min_quantity)
    .fetch_one(&mut **tx)
    .await?;

    Ok(method)
}

/// Adds a rule to one of the three sets. Eligibility rules hang off the
/// promotion; target and buy rules hang off its application method.
pub async fn add_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    kind: RuleKind,
    new: NewRule,
) -> Result<Rule> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    if new.attribute.trim().is_empty() || new.allowed_values.is_empty() {
        return Err(Error::invalid("a rule needs an attribute and a value"));
    }

    let statement = match kind {
        RuleKind::Eligibility => {
            "insert into promotion_rule
                 (id, scope, promotion_id, attribute, operator, allowed_values, description)
             values ($1, $2, $3, $4, $5, $6, $7)
             returning id, attribute, operator, allowed_values, description"
        }
        RuleKind::Target => {
            "insert into promotion_target_rule
                 (id, scope, application_method_id, attribute, operator, allowed_values,
                  description)
             select $1::uuid, $2::uuid, m.id, $4::text, $5::text, $6::text[], $7::text
             from application_method m
             where m.scope = $2 and m.promotion_id = $3
             returning id, attribute, operator, allowed_values, description"
        }
        RuleKind::Buy => {
            "insert into promotion_buy_rule
                 (id, scope, application_method_id, attribute, operator, allowed_values,
                  description)
             select $1::uuid, $2::uuid, m.id, $4::text, $5::text, $6::text[], $7::text
             from application_method m
             where m.scope = $2 and m.promotion_id = $3
             returning id, attribute, operator, allowed_values, description"
        }
    };

    sqlx::query_as::<_, Rule>(statement)
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .bind(new.attribute.trim())
        .bind(new.operator.as_str())
        .bind(&new.allowed_values)
        .bind(new.description.as_deref())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found("application method"))
}

/// The rules in one of the three sets. Configuration rather than a customer's
/// data, so it is capped rather than paged.
pub async fn rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    kind: RuleKind,
) -> Result<Vec<Rule>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    let statement = match kind {
        RuleKind::Eligibility => {
            "select id, attribute, operator, allowed_values, description
             from promotion_rule
             where scope = $1 and promotion_id = $2
             order by id
             limit $3"
        }
        RuleKind::Target => {
            "select r.id, r.attribute, r.operator, r.allowed_values, r.description
             from promotion_target_rule r
             join application_method m on m.id = r.application_method_id and m.scope = r.scope
             where r.scope = $1 and m.promotion_id = $2
             order by r.id
             limit $3"
        }
        RuleKind::Buy => {
            "select r.id, r.attribute, r.operator, r.allowed_values, r.description
             from promotion_buy_rule r
             join application_method m on m.id = r.application_method_id and m.scope = r.scope
             where r.scope = $1 and m.promotion_id = $2
             order by r.id
             limit $3"
        }
    };

    let rows = sqlx::query_as::<_, Rule>(statement)
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .bind(MAX_RULES)
        .fetch_all(&mut **tx)
        .await?;

    Ok(rows)
}

/// Takes a rule out of one of the three sets.
pub async fn remove_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    kind: RuleKind,
    rule_id: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    let statement = match kind {
        RuleKind::Eligibility => {
            "delete from promotion_rule
             where scope = $1 and promotion_id = $2 and id = $3"
        }
        RuleKind::Target => {
            "delete from promotion_target_rule r
             using application_method m
             where r.scope = $1
               and m.scope = $1
               and m.id = r.application_method_id
               and m.promotion_id = $2
               and r.id = $3"
        }
        RuleKind::Buy => {
            "delete from promotion_buy_rule r
             using application_method m
             where r.scope = $1
               and m.scope = $1
               and m.id = r.application_method_id
               and m.promotion_id = $2
               and r.id = $3"
        }
    };

    let done = sqlx::query(statement)
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .bind(rule_id)
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("rule"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "promotion rule",
            entity_id: rule_id,
            summary: serde_json::json!({ "promotion_id": promotion_id }),
        },
    )
    .await?;

    Ok(())
}

/// Takes one claim of a promotion, and what it costs off its campaign's budget:
/// one use of a `usage` budget, `spend` of a `spend` one.
///
/// `spend` is the discount this claim actually gave away. There is one entry
/// point rather than two, because a caller that could pick the cheaper one
/// eventually does and a spend budget is then never charged.
///
/// Statements that either move a counter or affect no row. Nothing is read
/// first: a read followed by a write is two checkouts both seeing the last one.
///
/// A refusal leaves the counter it already moved to be undone by the caller's
/// rollback, which is why both live in the caller's transaction.
///
/// [`Action::Settle`], not [`Action::Write`]: this spends a campaign's
/// budget, and editing a promotion's name is not the same power as spending
/// what it is allowed to give away.
pub async fn claim(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    customer_id: Option<CustomerId>,
    spend: Money,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    if spend.is_negative() {
        return Err(Error::invalid(
            "a promotion cannot give away less than nothing",
        ));
    }

    take(tx, ctx, promotion_id, customer_id, spend).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "promotion",
            entity_id: promotion_id.as_uuid(),
            summary: serde_json::json!({
                "spent": spend.amount.to_string(),
                "currency": spend.currency.as_str(),
                "customer_id": customer_id.map(CustomerId::as_uuid),
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "promotion.claimed",
            entity_id: promotion_id.as_uuid(),
            payload: serde_json::json!({
                "customer_id": customer_id.map(CustomerId::as_uuid),
            }),
        },
    )
    .await?;

    Ok(())
}

/// Gives a claim back, for a checkout that did not become an order.
///
/// The inverse of [`claim`], and clamped at zero rather than trusting the
/// caller: a compensation that ran twice must leave a promotion with fewer
/// uses than it started with in neither direction.
///
/// [`Action::Settle`] for the same reason [`claim`] asks for it: this gives
/// spent budget back.
pub async fn release(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    customer_id: Option<CustomerId>,
    spend: Money,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Promotion {
            id: Some(promotion_id.as_uuid()),
        },
    )?;

    let campaign_id: Option<Option<Uuid>> = sqlx::query_scalar(
        "update promotion
         set used = greatest(used - 1, 0)
         where scope = $1 and id = $2
         returning campaign_id",
    )
    .bind(ctx.scope.0)
    .bind(promotion_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some(campaign_id) = campaign_id else {
        return Ok(());
    };

    if let Some(customer_id) = customer_id {
        sqlx::query(
            "update promotion_usage
             set used = greatest(used - 1, 0)
             where scope = $1 and promotion_id = $2 and customer_id = $3",
        )
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .bind(customer_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    if let Some(campaign_id) = campaign_id {
        release_budget(
            tx,
            ctx,
            CampaignId::from_uuid(campaign_id),
            spend,
            customer_id.map(|id| id.to_string()).as_deref(),
        )
        .await?;
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "promotion",
            entity_id: promotion_id.as_uuid(),
            summary: serde_json::json!({
                "released": spend.amount.to_string(),
                "currency": spend.currency.as_str(),
                "customer_id": customer_id.map(CustomerId::as_uuid),
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "promotion.released",
            entity_id: promotion_id.as_uuid(),
            payload: serde_json::json!({
                "customer_id": customer_id.map(CustomerId::as_uuid),
            }),
        },
    )
    .await?;

    Ok(())
}

// `claim` asks; this only spends. A private helper with its own permit
// beside a caller that already asked would be asking twice.
async fn take(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
    customer_id: Option<CustomerId>,
    spend: Money,
) -> Result<()> {
    let taken = sqlx::query_as::<_, (Option<Uuid>,)>(
        "update promotion
         set used = used + 1
         where scope = $1
           and id = $2
           and deleted_at is null
           and status = 'active'
           and (usage_limit is null or used < usage_limit)
         returning campaign_id",
    )
    .bind(ctx.scope.0)
    .bind(promotion_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some((campaign_id,)) = taken else {
        return Err(Error::conflict("that promotion has no uses left"));
    };

    if let Some(customer_id) = customer_id {
        let mine = sqlx::query(
            r#"insert into promotion_usage
                   (id, scope, promotion_id, customer_id, used, "limit")
               select $1::uuid, $2::uuid, p.id, $4::uuid, 1, p.customer_usage_limit
               from promotion p
               where p.scope = $2 and p.id = $3
               on conflict (scope, promotion_id, customer_id) do update
                 set used = promotion_usage.used + 1
                 where promotion_usage."limit" is null
                    or promotion_usage.used < promotion_usage."limit""#,
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .bind(customer_id.as_uuid())
        .execute(&mut **tx)
        .await?;

        if mine.rows_affected() == 0 {
            return Err(Error::conflict(
                "that customer has used this promotion already",
            ));
        }
    }

    if let Some(campaign_id) = campaign_id {
        charge_budget(
            tx,
            ctx,
            CampaignId::from_uuid(campaign_id),
            spend,
            customer_id.map(|id| id.to_string()).as_deref(),
        )
        .await?;
    }

    Ok(())
}

/// What a claim costs the campaign's budget, moved by one statement so a
/// caller can never see the counter between reading it and writing it back.
///
/// The budget's own kind decides what is charged — one use, or the money the
/// claim gave away — because a caller allowed to choose eventually charges the
/// cheaper of the two. `spend`/`usage` move `campaign_budget`'s own counter,
/// in aggregate across every customer; `use_by_attribute`/`spend_by_attribute`
/// move the row `attribute_value` owns in `campaign_budget_usage` instead —
/// one counter per customer (or whatever `attribute` names), shared by every
/// promotion the campaign runs. A claim that resolved to no attribute value
/// cannot charge a budget kept per attribute.
async fn charge_budget(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    spend: Money,
    attribute_value: Option<&str>,
) -> Result<()> {
    let budget: Option<(Uuid, String, Option<String>)> = sqlx::query_as(
        "select id, type, currency_code from campaign_budget where scope = $1 and campaign_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(campaign_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some((budget_id, kind, currency_code)) = budget else {
        return Ok(());
    };

    let spends =
        kind == BudgetKind::Spend.as_str() || kind == BudgetKind::SpendByAttribute.as_str();
    if spends && currency_code.as_deref() != Some(spend.currency.as_str()) {
        return Err(Error::bug(
            "a campaign's budget is kept in another currency",
        ));
    }
    let charge = if spends { spend.amount } else { Decimal::ONE };

    let moved = if kind == BudgetKind::UseByAttribute.as_str()
        || kind == BudgetKind::SpendByAttribute.as_str()
    {
        let Some(attribute_value) = attribute_value else {
            return Err(Error::invalid(
                "that campaign's budget is kept per attribute, and this claim named none",
            ));
        };

        // Guarded on the insert branch too, not only the conflict branch: a
        // first claim charging more than the whole cap must be refused, not
        // written and left for the check constraint to reject the statement.
        sqlx::query(
            r#"insert into campaign_budget_usage
                   (id, scope, campaign_budget_id, attribute_value, used, "limit")
               select $1::uuid, $2::uuid, b.id, $4::text, $5::numeric, b."limit"
               from campaign_budget b
               where b.scope = $2
                 and b.id = $3
                 and (b."limit" is null or $5::numeric <= b."limit")
               on conflict (scope, campaign_budget_id, attribute_value) do update
                 set used = campaign_budget_usage.used + $5::numeric
                 where campaign_budget_usage."limit" is null
                    or campaign_budget_usage.used + $5::numeric
                       <= campaign_budget_usage."limit""#,
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(budget_id)
        .bind(attribute_value)
        .bind(charge)
        .execute(&mut **tx)
        .await?
    } else {
        sqlx::query(
            r#"update campaign_budget
               set used = used + $3
               where scope = $1
                 and id = $2
                 and ("limit" is null or used + $3 <= "limit")"#,
        )
        .bind(ctx.scope.0)
        .bind(budget_id)
        .bind(charge)
        .execute(&mut **tx)
        .await?
    };

    if moved.rows_affected() == 0 {
        return Err(Error::conflict("that campaign's budget is spent"));
    }

    Ok(())
}

/// The inverse of [`charge_budget`], clamped at zero the way [`release`]
/// already is — a compensation that runs twice must not overdraw either
/// counter in the other direction.
async fn release_budget(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    campaign_id: CampaignId,
    spend: Money,
    attribute_value: Option<&str>,
) -> Result<()> {
    let budget: Option<(Uuid, String)> = sqlx::query_as(
        "select id, type from campaign_budget where scope = $1 and campaign_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(campaign_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let Some((budget_id, kind)) = budget else {
        return Ok(());
    };

    let spends =
        kind == BudgetKind::Spend.as_str() || kind == BudgetKind::SpendByAttribute.as_str();
    let charge = if spends { spend.amount } else { Decimal::ONE };

    if kind == BudgetKind::UseByAttribute.as_str() || kind == BudgetKind::SpendByAttribute.as_str()
    {
        let Some(attribute_value) = attribute_value else {
            return Ok(());
        };

        sqlx::query(
            r#"update campaign_budget_usage
               set used = greatest(used - $4, 0)
               where scope = $1 and campaign_budget_id = $2 and attribute_value = $3"#,
        )
        .bind(ctx.scope.0)
        .bind(budget_id)
        .bind(attribute_value)
        .bind(charge)
        .execute(&mut **tx)
        .await?;

        return Ok(());
    }

    sqlx::query(
        r#"update campaign_budget set used = greatest(used - $3, 0) where scope = $1 and id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(budget_id)
    .bind(charge)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

#[derive(Debug, Clone, FromRow)]
struct CartRow {
    id: CartId,
    customer_id: Option<Uuid>,
    email: Option<String>,
    region_id: Option<Uuid>,
    currency_code: String,
    sales_channel_id: Option<Uuid>,
    metadata: Option<serde_json::Value>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, FromRow)]
struct LineRow {
    id: LineItemId,
    variant_id: Option<Uuid>,
    product_id: Option<Uuid>,
    product_title: String,
    quantity: i32,
    unit_price: Decimal,
    currency_code: String,
    is_tax_inclusive: bool,
    is_discountable: bool,
}

#[derive(Debug, Clone, FromRow)]
struct MethodRow {
    id: ShippingMethodId,
    shipping_option_id: Option<Uuid>,
    name: String,
    amount: Decimal,
    currency_code: String,
    is_tax_inclusive: bool,
}

/// One thing a discount can land on, and what is left of it.
#[derive(Debug, Clone)]
struct Slot {
    target: AdjustmentTarget,
    remaining: Decimal,
    unit_price: Decimal,
    quantity: i32,
    is_tax_inclusive: bool,
    attributes: Vec<(String, String)>,
}

/// Works out what every promotion on this cart takes off, and writes the
/// adjustments.
///
/// Which promotions: the automatic ones, and the ones whose code the cart
/// carries under `metadata.promotions`.
///
/// In what order: automatic first, then codes, each group oldest first. The
/// order is fixed rather than incidental because each promotion works on what
/// the ones before it left, so a percentage taken second is a percentage of
/// less — and two carts with the same contents have to reach the same total.
///
/// One adjustment per slot a promotion takes, so bounded by the cart it is
/// applied to: [`crate::cart::MAX_LINES`] lines and [`MAX_SHIPPING_METHODS`]
/// methods.
pub async fn apply(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Vec<Adjustment>> {
    let cart = sqlx::query_as::<_, CartRow>(
        "select id, customer_id, email, region_id, currency_code, sales_channel_id, metadata,
                completed_at
         from cart
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    // No row to say whose cart this is yet, so the ask below is on nobody's
    // behalf — asked before answering even when there is nothing to answer
    // about.
    let Some(cart) = cart else {
        let _: Permit = ctx.permit(
            Action::Write,
            Resource::Cart {
                id: cart_id.as_uuid(),
                customer: None,
            },
        )?;
        return Err(Error::not_found("cart"));
    };

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: cart.customer_id,
        },
    )?;

    if cart.completed_at.is_some() {
        return Err(Error::conflict("that cart is already an order"));
    }

    let currency = Currency::parse(&cart.currency_code)?;
    let exponent = store::exponent(tx, ctx, currency).await?;

    let lines = sqlx::query_as::<_, LineRow>(
        "select id, variant_id, product_id, product_title, quantity, unit_price, currency_code,
                is_tax_inclusive, is_discountable
         from cart_line_item
         where scope = $1 and cart_id = $2
         order by created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(crate::cart::MAX_LINES)
    .fetch_all(&mut **tx)
    .await?;

    let methods = sqlx::query_as::<_, MethodRow>(
        "select id, shipping_option_id, name, amount, currency_code, is_tax_inclusive
         from cart_shipping_method
         where scope = $1 and cart_id = $2
         order by created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(MAX_SHIPPING_METHODS)
    .fetch_all(&mut **tx)
    .await?;

    clear_adjustments(tx, ctx, cart_id).await?;

    let mut line_slots: Vec<Slot> = lines
        .iter()
        .filter(|line| line.is_discountable)
        .map(slot_for_line)
        .collect();
    let mut method_slots: Vec<Slot> = methods.iter().map(slot_for_method).collect();

    let codes = codes_on(&cart);
    let candidates = candidates(tx, ctx, &codes).await?;

    let cart_context = cart_attributes(&cart, &lines);
    let mut adjustments = Vec::new();

    for promotion in candidates {
        let rules = rules_of(tx, ctx, RuleKind::Eligibility, promotion.id).await?;
        if !rules.iter().all(|rule| rule_holds(rule, &cart_context)) {
            continue;
        }

        let Some(method) = method_of(tx, ctx, promotion.id).await? else {
            continue;
        };

        let taken = apply_one(
            tx,
            ctx,
            &promotion,
            &method,
            &mut line_slots,
            &mut method_slots,
            currency,
            exponent,
        )
        .await?;

        adjustments.extend(taken);
    }

    for adjustment in &adjustments {
        write_adjustment(tx, ctx, adjustment, &line_slots, &method_slots).await?;
    }

    Ok(adjustments)
}

#[allow(clippy::too_many_arguments)]
async fn apply_one(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion: &Promotion,
    method: &ApplicationMethod,
    line_slots: &mut [Slot],
    method_slots: &mut [Slot],
    currency: Currency,
    exponent: u32,
) -> Result<Vec<Adjustment>> {
    let value = method.value.unwrap_or(Decimal::ZERO);
    if value.is_zero() {
        return Ok(Vec::new());
    }

    let kind = match method.kind.as_str() {
        "fixed" => MethodKind::Fixed,
        _ => MethodKind::Percentage,
    };
    let allocation = match method.allocation.as_deref() {
        Some("across") => Allocation::Across,
        Some("once") => Allocation::Once,
        _ => Allocation::Each,
    };

    let chosen: Vec<usize> = match method.target_type.as_str() {
        "shipping_methods" => {
            let rules = rules_of(tx, ctx, RuleKind::Target, promotion.id).await?;
            pick(method_slots, &rules)
        }
        "items" => {
            let rules = rules_of(tx, ctx, RuleKind::Target, promotion.id).await?;
            pick(line_slots, &rules)
        }
        // An order-wide discount lands on every line that may be discounted.
        _ => (0..line_slots.len()).collect(),
    };

    if promotion.kind == PromotionKind::BuyGet.as_str() {
        let buy_rules = rules_of(tx, ctx, RuleKind::Buy, promotion.id).await?;
        let bought: i32 = pick(line_slots, &buy_rules)
            .into_iter()
            .filter_map(|at| line_slots.get(at).map(|slot| slot.quantity))
            .sum();

        if bought < method.buy_rules_min_quantity.unwrap_or(1) {
            return Ok(Vec::new());
        }

        return Ok(buy_get(
            promotion, method, line_slots, &chosen, kind, value, currency, exponent,
        ));
    }

    let slots: &mut [Slot] = if method.target_type == "shipping_methods" {
        method_slots
    } else {
        line_slots
    };

    // A plain promotion reaching for the same cheapest-first quota `buy_get`
    // always applies, rather than a discount on every matched line or one
    // amount split between them.
    if allocation == Allocation::Once {
        return Ok(cheapest_units(
            promotion,
            slots,
            &chosen,
            method.max_quantity.unwrap_or(i32::MAX),
            kind,
            value,
            currency,
            exponent,
        ));
    }

    let chosen: Vec<usize> = chosen
        .into_iter()
        .filter(|at| {
            slots
                .get(*at)
                .is_some_and(|slot| slot.remaining > Decimal::ZERO)
        })
        .collect();

    if chosen.is_empty() {
        return Ok(Vec::new());
    }

    let amounts = match (kind, allocation) {
        (MethodKind::Percentage, Allocation::Each) => chosen
            .iter()
            .filter_map(|at| slots.get(*at))
            .map(|slot| (slot.remaining * value / Decimal::from(100)).round_dp(exponent))
            .collect::<Vec<Decimal>>(),
        (MethodKind::Fixed, Allocation::Each) => chosen
            .iter()
            .filter_map(|at| slots.get(*at))
            .map(|slot| value.min(slot.remaining).round_dp(exponent))
            .collect(),
        // `Once` always returns above; kept here only so the match stays
        // exhaustive over every `Allocation`.
        (_, Allocation::Once) => Vec::new(),
        (_, Allocation::Across) => {
            let base: Decimal = chosen
                .iter()
                .filter_map(|at| slots.get(*at))
                .map(|slot| slot.remaining)
                .sum();

            let total = match kind {
                MethodKind::Percentage => (base * value / Decimal::from(100)).round_dp(exponent),
                MethodKind::Fixed => value.min(base).round_dp(exponent),
            };

            let weights: Vec<Decimal> = chosen
                .iter()
                .filter_map(|at| slots.get(*at))
                .map(|slot| slot.remaining)
                .collect();

            // The remainder has to land somewhere: what the lines add up to is
            // what the provider is asked for.
            allocate(Money::new(total, currency), &weights, exponent)?
                .into_iter()
                .map(|part| part.amount)
                .collect()
        }
    };

    let mut out = Vec::new();
    for (at, amount) in chosen.into_iter().zip(amounts) {
        let Some(slot) = slots.get_mut(at) else {
            continue;
        };
        let amount = amount.min(slot.remaining);
        if amount <= Decimal::ZERO {
            continue;
        }
        slot.remaining -= amount;
        out.push(Adjustment {
            id: AdjustmentId::new(),
            promotion_id: promotion.id,
            code: promotion.code.clone(),
            target: slot.target,
            amount: Money::new(amount, currency),
            description: None,
        });
    }

    Ok(out)
}

/// Buy X, get Y: the cheapest units of the target are the ones given away, up
/// to `apply_to_quantity`.
#[allow(clippy::too_many_arguments)]
fn buy_get(
    promotion: &Promotion,
    method: &ApplicationMethod,
    slots: &mut [Slot],
    targets: &[usize],
    kind: MethodKind,
    value: Decimal,
    currency: Currency,
    exponent: u32,
) -> Vec<Adjustment> {
    cheapest_units(
        promotion,
        slots,
        targets,
        method.apply_to_quantity.unwrap_or(1),
        kind,
        value,
        currency,
        exponent,
    )
}

/// The cheapest units among `targets` are the ones the discount reaches, up to
/// `quota` units total across every line it lands on. `buy_get`'s free units
/// and a plain promotion's `Allocation::Once` are this same walk over a
/// different quota — `apply_to_quantity` for the first, `max_quantity` for
/// the second — so there is one implementation rather than two that have to
/// agree.
#[allow(clippy::too_many_arguments)]
fn cheapest_units(
    promotion: &Promotion,
    slots: &mut [Slot],
    targets: &[usize],
    quota: i32,
    kind: MethodKind,
    value: Decimal,
    currency: Currency,
    exponent: u32,
) -> Vec<Adjustment> {
    let mut order: Vec<usize> = targets
        .iter()
        .copied()
        .filter(|at| {
            slots
                .get(*at)
                .is_some_and(|slot| slot.remaining > Decimal::ZERO)
        })
        .collect();
    order.sort_by_key(|at| slots.get(*at).map(|slot| slot.unit_price));

    let mut left = quota;
    let mut out = Vec::new();

    for at in order {
        if left <= 0 {
            break;
        }
        let Some(slot) = slots.get_mut(at) else {
            continue;
        };

        let units = left.min(slot.quantity);
        let per_unit = match kind {
            MethodKind::Percentage => slot.unit_price * value / Decimal::from(100),
            MethodKind::Fixed => value.min(slot.unit_price),
        };

        let amount = (per_unit * Decimal::from(units))
            .round_dp(exponent)
            .min(slot.remaining);

        if amount <= Decimal::ZERO {
            continue;
        }

        slot.remaining -= amount;
        left -= units;
        out.push(Adjustment {
            id: AdjustmentId::new(),
            promotion_id: promotion.id,
            code: promotion.code.clone(),
            target: slot.target,
            amount: Money::new(amount, currency),
            description: None,
        });
    }

    out
}

async fn candidates(tx: &mut Tx<'_>, ctx: &Ctx<'_>, codes: &[String]) -> Result<Vec<Promotion>> {
    let rows = sqlx::query_as::<_, Promotion>(
        "select p.id, p.campaign_id, p.code, p.type, p.status, p.is_automatic, p.usage_limit,
                p.used, p.customer_usage_limit, p.metadata, p.created_at
         from promotion p
         left join campaign c on c.id = p.campaign_id and c.scope = p.scope
         where p.scope = $1
           and p.deleted_at is null
           and p.status = 'active'
           and (p.is_automatic or p.code = any($2))
           and (p.usage_limit is null or p.used < p.usage_limit)
           and (c.id is null
                or ((c.starts_at is null or c.starts_at <= $3)
                    and (c.ends_at is null or c.ends_at > $3)))
         order by p.is_automatic desc, p.created_at, p.id",
    )
    .bind(ctx.scope.0)
    .bind(codes)
    .bind(ctx.now())
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

async fn method_of(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    promotion_id: PromotionId,
) -> Result<Option<ApplicationMethod>> {
    let row = sqlx::query_as::<_, ApplicationMethod>(
        "select id, promotion_id, type, target_type, allocation, value, currency_code,
                max_quantity, apply_to_quantity, buy_rules_min_quantity
         from application_method
         where scope = $1 and promotion_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(promotion_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row)
}

async fn rules_of(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    kind: RuleKind,
    promotion_id: PromotionId,
) -> Result<Vec<Rule>> {
    let statement = match kind {
        RuleKind::Eligibility => {
            "select id, attribute, operator, allowed_values, description
             from promotion_rule
             where scope = $1 and promotion_id = $2
             order by id"
        }
        RuleKind::Target => {
            "select r.id, r.attribute, r.operator, r.allowed_values, r.description
             from promotion_target_rule r
             join application_method m on m.id = r.application_method_id and m.scope = r.scope
             where r.scope = $1 and m.promotion_id = $2
             order by r.id"
        }
        RuleKind::Buy => {
            "select r.id, r.attribute, r.operator, r.allowed_values, r.description
             from promotion_buy_rule r
             join application_method m on m.id = r.application_method_id and m.scope = r.scope
             where r.scope = $1 and m.promotion_id = $2
             order by r.id"
        }
    };

    let rows = sqlx::query_as::<_, Rule>(statement)
        .bind(ctx.scope.0)
        .bind(promotion_id.as_uuid())
        .fetch_all(&mut **tx)
        .await?;

    Ok(rows)
}

async fn clear_adjustments(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<()> {
    sqlx::query(
        "delete from cart_line_item_adjustment a
         using cart_line_item l
         where a.scope = $1
           and l.scope = $1
           and a.cart_line_item_id = l.id
           and l.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "delete from cart_shipping_method_adjustment a
         using cart_shipping_method m
         where a.scope = $1
           and m.scope = $1
           and a.cart_shipping_method_id = m.id
           and m.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn write_adjustment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    adjustment: &Adjustment,
    line_slots: &[Slot],
    method_slots: &[Slot],
) -> Result<()> {
    let inclusive = line_slots
        .iter()
        .chain(method_slots)
        .find(|slot| slot.target == adjustment.target)
        .is_some_and(|slot| slot.is_tax_inclusive);

    match adjustment.target {
        AdjustmentTarget::LineItem(id) => {
            sqlx::query(
                "insert into cart_line_item_adjustment
                     (id, scope, cart_line_item_id, promotion_id, code, amount, currency_code,
                      is_tax_inclusive)
                 values ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(adjustment.id.as_uuid())
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .bind(adjustment.promotion_id.as_uuid())
            .bind(&adjustment.code)
            .bind(adjustment.amount.amount)
            .bind(adjustment.amount.currency.as_str())
            .bind(inclusive)
            .execute(&mut **tx)
            .await?;
        }
        AdjustmentTarget::ShippingMethod(id) => {
            sqlx::query(
                "insert into cart_shipping_method_adjustment
                     (id, scope, cart_shipping_method_id, promotion_id, code, amount,
                      currency_code, is_tax_inclusive)
                 values ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(adjustment.id.as_uuid())
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .bind(adjustment.promotion_id.as_uuid())
            .bind(&adjustment.code)
            .bind(adjustment.amount.amount)
            .bind(adjustment.amount.currency.as_str())
            .bind(inclusive)
            .execute(&mut **tx)
            .await?;
        }
    }

    Ok(())
}

fn slot_for_line(line: &LineRow) -> Slot {
    let total = line.unit_price * Decimal::from(line.quantity);
    Slot {
        target: AdjustmentTarget::LineItem(line.id),
        remaining: total,
        unit_price: line.unit_price,
        quantity: line.quantity,
        is_tax_inclusive: line.is_tax_inclusive,
        attributes: vec![
            ("item_id".into(), line.id.to_string()),
            (
                "variant_id".into(),
                line.variant_id.map(|id| id.to_string()).unwrap_or_default(),
            ),
            (
                "product_id".into(),
                line.product_id.map(|id| id.to_string()).unwrap_or_default(),
            ),
            ("product_title".into(), line.product_title.clone()),
            ("currency_code".into(), line.currency_code.clone()),
            ("unit_price".into(), line.unit_price.to_string()),
            ("quantity".into(), line.quantity.to_string()),
        ],
    }
}

fn slot_for_method(method: &MethodRow) -> Slot {
    Slot {
        target: AdjustmentTarget::ShippingMethod(method.id),
        remaining: method.amount,
        unit_price: method.amount,
        quantity: 1,
        is_tax_inclusive: method.is_tax_inclusive,
        attributes: vec![
            ("shipping_method_id".into(), method.id.to_string()),
            (
                "shipping_option_id".into(),
                method
                    .shipping_option_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            ),
            ("name".into(), method.name.clone()),
            ("currency_code".into(), method.currency_code.clone()),
            ("amount".into(), method.amount.to_string()),
        ],
    }
}

fn cart_attributes(cart: &CartRow, lines: &[LineRow]) -> Vec<(String, String)> {
    let total: Decimal = lines
        .iter()
        .map(|line| line.unit_price * Decimal::from(line.quantity))
        .sum();
    let count: i64 = lines.iter().map(|line| i64::from(line.quantity)).sum();

    vec![
        ("cart_id".into(), cart.id.to_string()),
        (
            "customer_id".into(),
            cart.customer_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ),
        ("email".into(), cart.email.clone().unwrap_or_default()),
        (
            "region_id".into(),
            cart.region_id.map(|id| id.to_string()).unwrap_or_default(),
        ),
        (
            "sales_channel_id".into(),
            cart.sales_channel_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
        ),
        ("currency_code".into(), cart.currency_code.clone()),
        ("item_total".into(), total.to_string()),
        ("item_count".into(), count.to_string()),
    ]
}

/// The codes somebody typed, as the cart carries them.
fn codes_on(cart: &CartRow) -> Vec<String> {
    cart.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("promotions"))
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn pick(slots: &[Slot], rules: &[Rule]) -> Vec<usize> {
    (0..slots.len())
        .filter(|at| {
            slots
                .get(*at)
                .is_some_and(|slot| rules.iter().all(|rule| rule_holds(rule, &slot.attributes)))
        })
        .collect()
}

fn rule_holds(rule: &Rule, attributes: &[(String, String)]) -> bool {
    let Ok(operator) = Operator::parse(&rule.operator) else {
        return false;
    };
    let Some((_, actual)) = attributes.iter().find(|(name, _)| *name == rule.attribute) else {
        return false;
    };

    match operator {
        Operator::Eq | Operator::In => rule.allowed_values.iter().any(|one| one == actual),
        Operator::Ne => !rule.allowed_values.iter().any(|one| one == actual),
        Operator::Gt | Operator::Lt | Operator::Gte | Operator::Lte => {
            let Ok(actual) = actual.parse::<Decimal>() else {
                return false;
            };
            rule.allowed_values.iter().any(|one| {
                one.parse::<Decimal>().is_ok_and(|expected| match operator {
                    Operator::Gt => actual > expected,
                    Operator::Lt => actual < expected,
                    Operator::Gte => actual >= expected,
                    _ => actual <= expected,
                })
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn rule(operator: Operator, values: &[&str]) -> Rule {
        Rule {
            id: Uuid::now_v7(),
            attribute: "item_total".into(),
            operator: operator.as_str().into(),
            allowed_values: values.iter().map(|value| (*value).to_string()).collect(),
            description: None,
        }
    }

    #[test]
    fn a_numeric_rule_compares_as_a_number_rather_than_as_text() {
        let attributes = vec![("item_total".to_string(), "100".to_string())];
        assert!(rule_holds(&rule(Operator::Gte, &["90"]), &attributes));
        assert!(!rule_holds(&rule(Operator::Gt, &["100"]), &attributes));
    }

    #[test]
    fn a_rule_about_something_the_cart_does_not_have_does_not_hold() {
        let mut missing = rule(Operator::Eq, &["x"]);
        missing.attribute = "nothing_knows_this".into();
        assert!(!rule_holds(&missing, &[]));
    }

    #[test]
    fn an_amount_shared_across_lines_adds_back_up_to_the_amount() {
        let currency = Currency::parse("TRY").expect("a currency code");
        let parts = allocate(
            Money::new(dec!(10.00), currency),
            &[dec!(33.33), dec!(33.33), dec!(33.34)],
            2,
        )
        .expect("an allocation");
        let back: Decimal = parts.iter().map(|part| part.amount).sum();
        assert_eq!(back, dec!(10.00));
    }
}
