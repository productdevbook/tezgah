//! What a thing costs, to whom, in what currency, at what quantity.
//!
//! A price set is the handle other domains point at — a variant, a shipping
//! option — and every price hangs off one. Asking what something costs is
//! [`resolve`]: it takes the context a cart already knows (currency, quantity,
//! region, customer group, sales channel) and answers with both the amount to
//! charge and the amount to strike through.
//!
//! Resolution ranks candidates rather than filtering to one: the price whose
//! rules cover the whole context wins, then the one satisfying the most rules,
//! then the highest rule priority, and underneath everything the ruleless
//! default. `rules_count` is denormalised on the row so that ranking is an
//! index read rather than a count per candidate.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{PriceId, PriceListId, PriceSetId, RegionId, ShippingOptionId, VariantId};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};

/// The attribute names tezgah itself puts into a resolution context. A host's
/// own rules may use any other name through [`PriceContext::extra`].
pub const CURRENCY_ATTRIBUTE: &str = "currency_code";
pub const REGION_ATTRIBUTE: &str = "region_id";
pub const CUSTOMER_GROUP_ATTRIBUTE: &str = "customer_group_id";
pub const SALES_CHANNEL_ATTRIBUTE: &str = "sales_channel_id";

/// Rules on one price are configuration, and their order carries meaning, so
/// this is a ceiling rather than a cursor.
const MAX_PRICE_RULES: i64 = 200;

const LIST_TYPES: [&str; 2] = ["sale", "override"];
const LIST_STATUSES: [&str; 3] = ["draft", "active", "expired"];
const RULE_OPERATORS: [&str; 6] = ["eq", "in", "gt", "lt", "gte", "lte"];

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PriceSet {
    pub id: PriceSetId,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Price {
    pub id: PriceId,
    pub price_set_id: PriceSetId,
    pub price_list_id: Option<PriceListId>,
    pub title: Option<String>,
    pub amount: Decimal,
    pub currency_code: String,
    pub min_quantity: Option<i32>,
    pub max_quantity: Option<i32>,
    pub rules_count: i32,
    pub created_at: DateTime<Utc>,
}

impl Price {
    pub fn money(&self) -> Result<Money> {
        Ok(Money::new(
            self.amount,
            Currency::parse(&self.currency_code)?,
        ))
    }
}

#[derive(Debug, Clone)]
pub struct NewPrice {
    pub price_set_id: PriceSetId,
    pub price_list_id: Option<PriceListId>,
    pub title: Option<String>,
    pub amount: Money,
    pub min_quantity: Option<i32>,
    pub max_quantity: Option<i32>,
    pub rules: Vec<NewPriceRule>,
}

/// Every field optional: what is left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct PriceUpdate {
    pub title: Option<String>,
    pub amount: Option<Money>,
    pub min_quantity: Option<Option<i32>>,
    pub max_quantity: Option<Option<i32>>,
}

#[derive(Debug, Clone)]
pub struct NewPriceRule {
    pub attribute: String,
    pub value: String,
    pub operator: String,
    pub priority: i32,
}

impl NewPriceRule {
    /// The common case: this attribute equals this value.
    pub fn eq(attribute: impl Into<String>, value: impl Into<String>) -> Self {
        NewPriceRule {
            attribute: attribute.into(),
            value: value.into(),
            operator: "eq".into(),
            priority: 0,
        }
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PriceRule {
    pub id: Uuid,
    pub price_id: PriceId,
    pub attribute: String,
    pub value: String,
    pub operator: String,
    pub priority: i32,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PriceList {
    pub id: PriceListId,
    pub title: String,
    pub description: Option<String>,
    /// `sale` or `override`. A sale is a temporary reduction and is left out of
    /// the original amount; an override replaces the price outright.
    #[sqlx(rename = "type")]
    pub kind: String,
    pub status: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
    pub rules_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewPriceList {
    pub title: String,
    pub description: Option<String>,
    pub kind: String,
    pub status: String,
    pub starts_at: Option<DateTime<Utc>>,
    pub ends_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct PriceListUpdate {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub starts_at: Option<Option<DateTime<Utc>>>,
    pub ends_at: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PriceListRule {
    pub id: Uuid,
    pub price_list_id: PriceListId,
    pub attribute: String,
    pub allowed_values: Vec<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PricePreference {
    pub id: Uuid,
    pub attribute: String,
    pub value: Option<String>,
    pub is_tax_inclusive: bool,
}

/// Everything resolution is allowed to know. Anything else a host prices by
/// goes in `extra` as an (attribute, value) pair matching a rule's attribute.
#[derive(Debug, Clone)]
pub struct PriceContext {
    pub currency: Currency,
    pub quantity: i32,
    pub region_id: Option<RegionId>,
    pub customer_group_id: Option<Uuid>,
    pub sales_channel_id: Option<Uuid>,
    pub extra: Vec<(String, String)>,
}

impl PriceContext {
    pub fn new(currency: Currency, quantity: i32) -> Self {
        PriceContext {
            currency,
            quantity,
            region_id: None,
            customer_group_id: None,
            sales_channel_id: None,
            extra: Vec::new(),
        }
    }

    pub fn in_region(mut self, region: RegionId) -> Self {
        self.region_id = Some(region);
        self
    }

    pub fn for_group(mut self, group: Uuid) -> Self {
        self.customer_group_id = Some(group);
        self
    }

    pub fn through(mut self, channel: Uuid) -> Self {
        self.sales_channel_id = Some(channel);
        self
    }

    pub fn with(mut self, attribute: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push((attribute.into(), value.into()));
        self
    }

    /// The context flattened the way both a price rule and a price list rule
    /// read it.
    pub fn pairs(&self) -> Vec<(String, String)> {
        let mut pairs = vec![(
            CURRENCY_ATTRIBUTE.to_string(),
            self.currency.as_str().to_string(),
        )];
        if let Some(region) = self.region_id {
            pairs.push((REGION_ATTRIBUTE.to_string(), region.to_string()));
        }
        if let Some(group) = self.customer_group_id {
            pairs.push((CUSTOMER_GROUP_ATTRIBUTE.to_string(), group.to_string()));
        }
        if let Some(channel) = self.sales_channel_id {
            pairs.push((SALES_CHANNEL_ATTRIBUTE.to_string(), channel.to_string()));
        }
        pairs.extend(self.extra.iter().cloned());
        pairs
    }
}

/// What to charge, and what to show struck through beside it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatedPrice {
    pub calculated: Money,
    pub original: Money,
    pub price_id: PriceId,
    pub price_list_id: Option<PriceListId>,
    /// The price the original came from, which is a different row whenever a
    /// sale list won the calculated one.
    pub original_price_id: PriceId,
}

impl CalculatedPrice {
    pub fn is_reduced(&self) -> bool {
        self.calculated.amount < self.original.amount
    }
}

#[derive(Debug, FromRow)]
struct ResolvedRow {
    price_id: Option<PriceId>,
    amount: Option<Decimal>,
    price_list_id: Option<PriceListId>,
    original_price_id: Option<PriceId>,
    original_amount: Option<Decimal>,
}

pub async fn create_price_set(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<PriceSet> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    let id = PriceSetId::new();
    let set = sqlx::query_as::<_, PriceSet>(
        "insert into price_set (id, scope) values ($1, $2) returning id, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_set",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(set)
}

pub async fn price_set(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PriceSetId) -> Result<PriceSet> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, PriceSet>(
        "select id, created_at from price_set where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("price set"))
}

/// Points a variant at a price set, replacing whatever it pointed at before.
pub async fn link_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
    set: PriceSetId,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    sqlx::query(
        "insert into product_variant_price_set (id, scope, variant_id, price_set_id)
         values ($1, $2, $3, $4)
         on conflict (scope, variant_id) do update set price_set_id = excluded.price_set_id",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(variant.as_uuid())
    .bind(set.as_uuid())
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "product_variant_price_set",
            entity_id: variant.as_uuid(),
            summary: serde_json::json!({ "price_set_id": set.as_uuid() }),
        },
    )
    .await?;

    Ok(())
}

pub async fn link_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    option: ShippingOptionId,
    set: PriceSetId,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    sqlx::query(
        "insert into shipping_option_price_set (id, scope, shipping_option_id, price_set_id)
         values ($1, $2, $3, $4)
         on conflict (scope, shipping_option_id)
         do update set price_set_id = excluded.price_set_id",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(option.as_uuid())
    .bind(set.as_uuid())
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "shipping_option_price_set",
            entity_id: option.as_uuid(),
            summary: serde_json::json!({ "price_set_id": set.as_uuid() }),
        },
    )
    .await?;

    Ok(())
}

pub async fn price_set_for_variant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant: VariantId,
) -> Result<Option<PriceSetId>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let found: Option<PriceSetId> = sqlx::query_scalar(
        "select price_set_id from product_variant_price_set
         where scope = $1 and variant_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(variant.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found)
}

pub async fn price_set_for_shipping_option(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    option: ShippingOptionId,
) -> Result<Option<PriceSetId>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let found: Option<PriceSetId> = sqlx::query_scalar(
        "select price_set_id from shipping_option_price_set
         where scope = $1 and shipping_option_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(option.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found)
}

pub async fn add_price(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewPrice) -> Result<Price> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if new.amount.amount.is_sign_negative() {
        return Err(Error::invalid("a price cannot be negative"));
    }
    for rule in &new.rules {
        check_rule(rule)?;
    }

    let id = PriceId::new();
    let rules_count = i32::try_from(new.rules.len())
        .map_err(|_| Error::invalid("that is more rules than a price can carry"))?;

    let price = sqlx::query_as::<_, Price>(
        "insert into price (id, scope, price_set_id, price_list_id, title, amount,
                            currency_code, min_quantity, max_quantity, rules_count)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         returning id, price_set_id, price_list_id, title, amount, currency_code,
                   min_quantity, max_quantity, rules_count, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.price_set_id.as_uuid())
    .bind(new.price_list_id.map(PriceListId::as_uuid))
    .bind(new.title.as_deref())
    .bind(new.amount.amount)
    .bind(new.amount.currency.as_str())
    .bind(new.min_quantity)
    .bind(new.max_quantity)
    .bind(rules_count)
    .fetch_one(&mut **tx)
    .await?;

    for rule in &new.rules {
        insert_rule(tx, ctx, id, rule).await?;
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "price_set_id": new.price_set_id.as_uuid(),
                "amount": new.amount.amount.to_string(),
                "currency_code": new.amount.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(price)
}

pub async fn update_price(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceId,
    change: PriceUpdate,
) -> Result<Price> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if let Some(amount) = change.amount {
        if amount.amount.is_sign_negative() {
            return Err(Error::invalid("a price cannot be negative"));
        }
    }

    let price = sqlx::query_as::<_, Price>(
        "update price
            set title        = coalesce($3, title),
                amount       = coalesce($4, amount),
                currency_code = coalesce($5, currency_code),
                min_quantity = case when $6 then $7 else min_quantity end,
                max_quantity = case when $8 then $9 else max_quantity end
          where scope = $1 and id = $2 and deleted_at is null
         returning id, price_set_id, price_list_id, title, amount, currency_code,
                   min_quantity, max_quantity, rules_count, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(change.title.as_deref())
    .bind(change.amount.map(|money| money.amount))
    .bind(
        change
            .amount
            .map(|money| money.currency.as_str().to_string()),
    )
    .bind(change.min_quantity.is_some())
    .bind(change.min_quantity.flatten())
    .bind(change.max_quantity.is_some())
    .bind(change.max_quantity.flatten())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("price"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "amount": price.amount.to_string() }),
        },
    )
    .await?;

    Ok(price)
}

pub async fn delete_price(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PriceId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Pricing)?;

    let gone = sqlx::query(
        "update price set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if gone.rows_affected() == 0 {
        return Err(Error::not_found("price"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "price",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

pub async fn prices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    set: PriceSetId,
    paging: Paging,
) -> Result<Page<Price>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, Price>(
        "select id, price_set_id, price_list_id, title, amount, currency_code,
                min_quantity, max_quantity, rules_count, created_at
         from price
         where scope = $1
           and price_set_id = $2
           and deleted_at is null
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(set.as_uuid())
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

/// Adds a rule and brings `rules_count` back in step with the rows, because
/// resolution ranks on the column rather than on a count.
pub async fn add_price_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
    rule: NewPriceRule,
) -> Result<PriceRule> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;
    check_rule(&rule)?;

    let written = insert_rule(tx, ctx, price, &rule).await?;
    recount_price_rules(tx, ctx, price).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_rule",
            entity_id: written.id,
            summary: serde_json::json!({
                "price_id": price.as_uuid(),
                "attribute": rule.attribute,
            }),
        },
    )
    .await?;

    Ok(written)
}

pub async fn remove_price_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
    rule: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Pricing)?;

    let gone = sqlx::query("delete from price_rule where scope = $1 and price_id = $2 and id = $3")
        .bind(ctx.scope.0)
        .bind(price.as_uuid())
        .bind(rule)
        .execute(&mut **tx)
        .await?;

    if gone.rows_affected() == 0 {
        return Err(Error::not_found("price rule"));
    }

    recount_price_rules(tx, ctx, price).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "price_rule",
            entity_id: rule,
            summary: serde_json::json!({ "price_id": price.as_uuid() }),
        },
    )
    .await?;

    Ok(())
}

pub async fn price_rules(tx: &mut Tx<'_>, ctx: &Ctx<'_>, price: PriceId) -> Result<Vec<PriceRule>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, PriceRule>(
        "select id, price_id, attribute, value, operator, priority
         from price_rule
         where scope = $1 and price_id = $2
         order by priority desc, attribute
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(price.as_uuid())
    .bind(MAX_PRICE_RULES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn create_price_list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewPriceList,
) -> Result<PriceList> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if new.title.trim().is_empty() {
        return Err(Error::invalid("a price list needs a title"));
    }
    if !LIST_TYPES.contains(&new.kind.as_str()) {
        return Err(Error::invalid("a price list is a sale or an override"));
    }
    if !LIST_STATUSES.contains(&new.status.as_str()) {
        return Err(Error::invalid("a price list is draft, active or expired"));
    }
    if let (Some(starts), Some(ends)) = (new.starts_at, new.ends_at) {
        if starts >= ends {
            return Err(Error::invalid("a price list ends after it starts"));
        }
    }

    let id = PriceListId::new();
    let list = sqlx::query_as::<_, PriceList>(
        "insert into price_list (id, scope, title, description, type, status, starts_at, ends_at)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, title, description, type, status, starts_at, ends_at,
                   rules_count, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.title.trim())
    .bind(new.description.as_deref())
    .bind(&new.kind)
    .bind(&new.status)
    .bind(new.starts_at)
    .bind(new.ends_at)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_list",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "title": list.title, "type": list.kind }),
        },
    )
    .await?;

    Ok(list)
}

pub async fn update_price_list(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PriceListId,
    change: PriceListUpdate,
) -> Result<PriceList> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if let Some(status) = &change.status {
        if !LIST_STATUSES.contains(&status.as_str()) {
            return Err(Error::invalid("a price list is draft, active or expired"));
        }
    }

    let list = sqlx::query_as::<_, PriceList>(
        "update price_list
            set title       = coalesce($3, title),
                description = coalesce($4, description),
                status      = coalesce($5, status),
                starts_at   = case when $6 then $7 else starts_at end,
                ends_at     = case when $8 then $9 else ends_at end
          where scope = $1 and id = $2 and deleted_at is null
         returning id, title, description, type, status, starts_at, ends_at,
                   rules_count, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(change.title.as_deref())
    .bind(change.description.as_deref())
    .bind(change.status.as_deref())
    .bind(change.starts_at.is_some())
    .bind(change.starts_at.flatten())
    .bind(change.ends_at.is_some())
    .bind(change.ends_at.flatten())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("price list"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_list",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "status": list.status }),
        },
    )
    .await?;

    Ok(list)
}

pub async fn add_price_list_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    list: PriceListId,
    attribute: impl Into<String>,
    allowed_values: Vec<String>,
) -> Result<PriceListRule> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    let attribute = attribute.into();
    if attribute.trim().is_empty() {
        return Err(Error::invalid("a price list rule needs an attribute"));
    }
    if allowed_values.is_empty() {
        return Err(Error::invalid("a price list rule needs a value to allow"));
    }

    let rule = sqlx::query_as::<_, PriceListRule>(
        "insert into price_list_rule (id, scope, price_list_id, attribute, allowed_values)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, price_list_id, attribute)
         do update set allowed_values = excluded.allowed_values
         returning id, price_list_id, attribute, allowed_values",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(list.as_uuid())
    .bind(attribute.trim())
    .bind(&allowed_values)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        "update price_list
            set rules_count = (select count(*) from price_list_rule
                               where scope = $1 and price_list_id = $2)
          where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(list.as_uuid())
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_list_rule",
            entity_id: rule.id,
            summary: serde_json::json!({
                "price_list_id": list.as_uuid(),
                "attribute": rule.attribute,
            }),
        },
    )
    .await?;

    Ok(rule)
}

pub async fn price_list(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PriceListId) -> Result<PriceList> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, PriceList>(
        "select id, title, description, type, status, starts_at, ends_at,
                rules_count, created_at
         from price_list
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("price list"))
}

pub async fn price_lists(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<PriceList>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, PriceList>(
        "select id, title, description, type, status, starts_at, ends_at,
                rules_count, created_at
         from price_list
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

pub async fn set_price_preference(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    attribute: impl Into<String>,
    value: Option<String>,
    is_tax_inclusive: bool,
) -> Result<PricePreference> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    let attribute = attribute.into();
    if attribute.trim().is_empty() {
        return Err(Error::invalid("a price preference needs an attribute"));
    }

    let preference = sqlx::query_as::<_, PricePreference>(
        "insert into price_preference (id, scope, attribute, value, is_tax_inclusive)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, attribute, coalesce(value, ''))
         do update set is_tax_inclusive = excluded.is_tax_inclusive
         returning id, attribute, value, is_tax_inclusive",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(attribute.trim())
    .bind(value.as_deref())
    .bind(is_tax_inclusive)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "price_preference",
            entity_id: preference.id,
            summary: serde_json::json!({
                "attribute": preference.attribute,
                "is_tax_inclusive": is_tax_inclusive,
            }),
        },
    )
    .await?;

    Ok(preference)
}

pub async fn price_preference(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    attribute: &str,
    value: Option<&str>,
) -> Result<Option<PricePreference>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let found = sqlx::query_as::<_, PricePreference>(
        "select id, attribute, value, is_tax_inclusive
         from price_preference
         where scope = $1 and attribute = $2 and coalesce(value, '') = coalesce($3, '')",
    )
    .bind(ctx.scope.0)
    .bind(attribute)
    .bind(value)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found)
}

/// Whether the amounts resolved in this context already carry tax.
///
/// The most specific preference wins: the region's answer over the currency's,
/// and no preference at all means they do not.
pub async fn is_tax_inclusive(tx: &mut Tx<'_>, ctx: &Ctx<'_>, at: &PriceContext) -> Result<bool> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let pairs = at.pairs();
    let attributes: Vec<String> = pairs.iter().map(|(a, _)| a.clone()).collect();
    let values: Vec<String> = pairs.iter().map(|(_, v)| v.clone()).collect();

    let found: Option<bool> = sqlx::query_scalar(
        "select p.is_tax_inclusive
         from price_preference p
         join unnest($2::text[], $3::text[]) as ctx (attribute, value)
           on ctx.attribute = p.attribute and ctx.value = coalesce(p.value, ctx.value)
         where p.scope = $1
         order by case p.attribute
                    when 'region_id' then 0
                    when 'currency_code' then 1
                    else 2
                  end,
                  (p.value is not null) desc
         limit 1",
    )
    .bind(ctx.scope.0)
    .bind(&attributes)
    .bind(&values)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found.unwrap_or(false))
}

/// What this price set costs in this context, and what to strike through.
///
/// One query: the candidates are gathered once and ranked twice, the second
/// ranking with the sale lists left out, so the original amount is the price
/// the shop would have charged rather than the sale price repeated.
pub async fn resolve(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    set: PriceSetId,
    at: &PriceContext,
) -> Result<Option<CalculatedPrice>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    if at.quantity <= 0 {
        return Err(Error::invalid("a quantity is a whole number above zero"));
    }

    let pairs = at.pairs();
    let attributes: Vec<String> = pairs.iter().map(|(a, _)| a.clone()).collect();
    let values: Vec<String> = pairs.iter().map(|(_, v)| v.clone()).collect();

    let row = sqlx::query_as::<_, ResolvedRow>(RESOLVE.as_str())
        .bind(ctx.scope.0)
        .bind(set.as_uuid())
        .bind(&attributes)
        .bind(&values)
        .bind(ctx.now())
        .bind(at.currency.as_str())
        .bind(at.quantity)
        .fetch_optional(&mut **tx)
        .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let (Some(price_id), Some(amount)) = (row.price_id, row.amount) else {
        return Ok(None);
    };

    let original_price_id = row.original_price_id.unwrap_or(price_id);
    let original_amount = row.original_amount.unwrap_or(amount);

    Ok(Some(CalculatedPrice {
        calculated: Money::new(amount, at.currency),
        original: Money::new(original_amount, at.currency),
        price_id,
        price_list_id: row.price_list_id,
        original_price_id,
    }))
}

const RANKING: &str = "exact desc, rules_count desc, priority desc, \
                       (price_list_id is not null) desc, amount, id";

/// Built once from [`RANKING`] so the two rankings cannot drift apart.
static RESOLVE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        r#"
with ctx (attribute, value) as (
    select * from unnest($3::text[], $4::text[])
),
live_list as (
    select l.id, l.type
    from price_list l
    where l.scope = $1
      and l.deleted_at is null
      and l.status = 'active'
      and (l.starts_at is null or l.starts_at <= $5)
      and (l.ends_at is null or l.ends_at > $5)
      and not exists (
          select 1
          from price_list_rule r
          where r.scope = $1
            and r.price_list_id = l.id
            and not exists (
                select 1 from ctx
                where ctx.attribute = r.attribute
                  and ctx.value = any (r.allowed_values)
            )
      )
),
candidate as (
    select p.id,
           p.amount,
           p.price_list_id,
           l.type as list_type,
           p.rules_count,
           p.rules_count = (select count(*) from ctx) as exact,
           coalesce((select max(pr.priority) from price_rule pr
                     where pr.scope = $1 and pr.price_id = p.id), 0) as priority
    from price p
    left join live_list l on l.id = p.price_list_id
    where p.scope = $1
      and p.price_set_id = $2
      and p.currency_code = $6
      and p.deleted_at is null
      and (p.price_list_id is null or l.id is not null)
      and (p.min_quantity is null or p.min_quantity <= $7)
      and (p.max_quantity is null or p.max_quantity >= $7)
      and not exists (
          select 1
          from price_rule pr
          where pr.scope = $1
            and pr.price_id = p.id
            and not exists (
                select 1 from ctx
                where ctx.attribute = pr.attribute
                  and case pr.operator
                        when 'eq' then ctx.value = pr.value
                        when 'in' then ctx.value = any (string_to_array(pr.value, ','))
                        else case
                            when ctx.value ~ '^-?[0-9]+(\.[0-9]+)?$'
                             and pr.value ~ '^-?[0-9]+(\.[0-9]+)?$'
                            then case pr.operator
                                when 'gt'  then ctx.value::numeric >  pr.value::numeric
                                when 'gte' then ctx.value::numeric >= pr.value::numeric
                                when 'lt'  then ctx.value::numeric <  pr.value::numeric
                                else            ctx.value::numeric <= pr.value::numeric
                            end
                            else false
                        end
                      end
            )
      )
),
best as (
    select id, amount, price_list_id,
           row_number() over (order by {RANKING}) as rank
    from candidate
),
best_original as (
    select id, amount,
           row_number() over (order by {RANKING}) as rank
    from candidate
    where price_list_id is null or list_type <> 'sale'
)
select c.id as price_id,
       c.amount as amount,
       c.price_list_id as price_list_id,
       o.id as original_price_id,
       o.amount as original_amount
from (select * from best where rank = 1) c
full join (select * from best_original where rank = 1) o on true
"#
    )
});

fn check_rule(rule: &NewPriceRule) -> Result<()> {
    if rule.attribute.trim().is_empty() {
        return Err(Error::invalid("a price rule needs an attribute"));
    }
    if rule.value.trim().is_empty() {
        return Err(Error::invalid("a price rule needs a value"));
    }
    if !RULE_OPERATORS.contains(&rule.operator.as_str()) {
        return Err(Error::invalid(format!(
            "{:?} is not a price rule operator",
            rule.operator
        )));
    }
    Ok(())
}

async fn insert_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    price: PriceId,
    rule: &NewPriceRule,
) -> Result<PriceRule> {
    let written = sqlx::query_as::<_, PriceRule>(
        "insert into price_rule (id, scope, price_id, attribute, value, operator, priority)
         values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (scope, price_id, attribute, operator)
         do update set value = excluded.value, priority = excluded.priority
         returning id, price_id, attribute, value, operator, priority",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(price.as_uuid())
    .bind(rule.attribute.trim())
    .bind(rule.value.trim())
    .bind(&rule.operator)
    .bind(rule.priority)
    .fetch_one(&mut **tx)
    .await?;

    Ok(written)
}

async fn recount_price_rules(tx: &mut Tx<'_>, ctx: &Ctx<'_>, price: PriceId) -> Result<()> {
    sqlx::query(
        "update price
            set rules_count = (select count(*) from price_rule
                               where scope = $1 and price_id = $2)
          where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(price.as_uuid())
    .execute(&mut **tx)
    .await?;

    Ok(())
}
