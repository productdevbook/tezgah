//! What a jurisdiction takes, and out of which amount.
//!
//! A region is a country with provinces hanging under it, and a rate belongs to
//! one region. A rate with no rule is the region's default; a rate with rules
//! applies only to what the rules name, and the most specific region that has
//! anything to say is the one that answers.
//!
//! Calculation writes nothing. It reads the configuration and returns lines,
//! and whoever asked stores them beside the cart or the order they belong to —
//! a tax line on a cart is a snapshot of an answer, not a view of this table.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{CartId, TaxRateId, TaxRegionId};
use crate::money::{Money, allocate};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};
use crate::store;

/// Tax is configuration: the regions an address falls in, the rates under them
/// and the rules on a rate are all small and all wanted whole, so each is
/// capped rather than paged — a page would silently tax a line at the wrong
/// rate.
const MAX_TAX_REGIONS: i64 = 100;
const MAX_TAX_RATES: i64 = 500;
const MAX_TAX_RATE_RULES: i64 = 1_000;

/// The kind of thing a rate rule points at. The row it names lives in another
/// domain, which is why the schema carries no foreign key for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaxReference {
    Product,
    ProductType,
    ProductCollection,
    ShippingOption,
}

impl TaxReference {
    pub fn as_str(self) -> &'static str {
        match self {
            TaxReference::Product => "product",
            TaxReference::ProductType => "product_type",
            TaxReference::ProductCollection => "product_collection",
            TaxReference::ShippingOption => "shipping_option",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "product" => Ok(TaxReference::Product),
            "product_type" => Ok(TaxReference::ProductType),
            "product_collection" => Ok(TaxReference::ProductCollection),
            "shipping_option" => Ok(TaxReference::ShippingOption),
            other => Err(Error::invalid(format!("{other:?} is not a tax reference"))),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaxRegion {
    pub id: TaxRegionId,
    pub country_code: String,
    pub province_code: Option<String>,
    pub parent_id: Option<TaxRegionId>,
    /// Names an outside authority that answers for this region instead of the
    /// rates below it.
    pub provider: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTaxRegion {
    pub country_code: String,
    pub province_code: Option<String>,
    pub parent_id: Option<TaxRegionId>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaxRate {
    pub id: TaxRateId,
    pub tax_region_id: TaxRegionId,
    /// A percentage: 18 is eighteen percent.
    pub rate: Decimal,
    pub code: Option<String>,
    pub name: String,
    pub is_default: bool,
    /// Whether this rate stacks on top of the region's default rather than
    /// replacing it.
    pub is_combinable: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTaxRate {
    pub tax_region_id: TaxRegionId,
    pub rate: Decimal,
    pub code: Option<String>,
    pub name: String,
    pub is_default: bool,
    pub is_combinable: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaxRateRuleRow {
    pub id: Uuid,
    pub tax_rate_id: TaxRateId,
    pub reference: String,
    pub reference_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct NewTaxRateRule {
    pub tax_rate_id: TaxRateId,
    pub reference: TaxReference,
    pub reference_id: Uuid,
}

/// Where the goods are going, as far as tax cares.
#[derive(Debug, Clone)]
pub struct TaxableAddress {
    pub country_code: String,
    pub province_code: Option<String>,
}

/// One thing a rate rule can match this line on.
#[derive(Debug, Clone, Copy)]
pub struct TaxTarget {
    pub reference: TaxReference,
    pub id: Uuid,
}

/// A line to be taxed. `amount` is the whole line: including tax when the shop
/// prices inclusively, excluding it when it does not.
#[derive(Debug, Clone)]
pub struct TaxableLine {
    pub id: Uuid,
    pub amount: Money,
    pub targets: Vec<TaxTarget>,
}

/// One rate's share of one line.
#[derive(Debug, Clone)]
pub struct TaxLine {
    pub line_id: Uuid,
    pub tax_rate_id: Option<TaxRateId>,
    pub code: String,
    pub name: String,
    pub rate: Decimal,
    pub amount: Money,
    pub is_tax_inclusive: bool,
}

/// For a shop whose tax is worked out somewhere else. A region naming a
/// provider is a region tezgah does not answer for.
#[async_trait]
pub trait TaxProvider: Send + Sync {
    fn code(&self) -> &'static str;

    async fn tax_lines(
        &self,
        lines: &[TaxableLine],
        address: &TaxableAddress,
        is_tax_inclusive: bool,
    ) -> Result<Vec<TaxLine>>;
}

pub async fn create_tax_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewTaxRegion,
) -> Result<TaxRegion> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    let country = country_code(&new.country_code)?;
    let province = new
        .province_code
        .as_ref()
        .map(|code| code.trim().to_string());
    if new.parent_id.is_some() && province.is_none() {
        return Err(Error::invalid("a region under a country needs a province"));
    }

    let id = TaxRegionId::new();
    let region = sqlx::query_as::<_, TaxRegion>(
        "insert into tax_region (id, scope, country_code, province_code, parent_id, provider)
         values ($1, $2, $3, $4, $5, $6)
         returning id, country_code, province_code, parent_id, provider, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(&country)
    .bind(province.as_deref())
    .bind(new.parent_id.map(TaxRegionId::as_uuid))
    .bind(new.provider.as_deref())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_region",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "country_code": region.country_code,
                "province_code": region.province_code,
            }),
        },
    )
    .await?;

    Ok(region)
}

pub async fn tax_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRegionId) -> Result<TaxRegion> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    sqlx::query_as::<_, TaxRegion>(
        "select id, country_code, province_code, parent_id, provider, created_at
         from tax_region
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tax region"))
}

/// A field left `None` is left alone. A region's parent is not among them: it
/// is what decides which rates apply, and moving it silently reprices orders
/// that have already been taxed.
#[derive(Debug, Clone, Default)]
pub struct TaxRegionPatch {
    pub country_code: Option<String>,
    pub province_code: Option<String>,
    pub provider: Option<String>,
}

pub async fn update_tax_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: TaxRegionId,
    patch: TaxRegionPatch,
) -> Result<TaxRegion> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    let country = patch
        .country_code
        .as_deref()
        .map(country_code)
        .transpose()?;

    let region = sqlx::query_as::<_, TaxRegion>(
        "update tax_region set
             country_code = coalesce($3::text, country_code),
             province_code = coalesce($4::text, province_code),
             provider = coalesce($5::text, provider)
         where scope = $1 and id = $2
         returning id, country_code, province_code, parent_id, provider, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(country)
    .bind(patch.province_code.as_deref().map(str::trim))
    .bind(patch.provider.as_deref())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tax region"))?;

    if region.parent_id.is_some() && region.province_code.is_none() {
        return Err(Error::invalid("a region under a country needs a province"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_region",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "country_code": region.country_code,
                "province_code": region.province_code,
            }),
        },
    )
    .await?;

    Ok(region)
}

pub async fn tax_regions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<TaxRegion>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let rows = sqlx::query_as::<_, TaxRegion>(
        "select id, country_code, province_code, parent_id, provider, created_at
         from tax_region
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

pub async fn delete_tax_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRegionId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Tax)?;

    let done = sqlx::query("delete from tax_region where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("tax region"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "tax_region",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

pub async fn create_tax_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewTaxRate) -> Result<TaxRate> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a tax rate needs a name"));
    }
    if new.rate.is_sign_negative() || new.rate > Decimal::from(100) {
        return Err(Error::invalid(
            "a tax rate is a percentage between 0 and 100",
        ));
    }

    let id = TaxRateId::new();
    let rate = sqlx::query_as::<_, TaxRate>(
        "insert into tax_rate
             (id, scope, tax_region_id, rate, code, name, is_default, is_combinable)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, tax_region_id, rate, code, name, is_default, is_combinable, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.tax_region_id.as_uuid())
    .bind(new.rate)
    .bind(new.code.as_deref())
    .bind(new.name.trim())
    .bind(new.is_default)
    .bind(new.is_combinable)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_rate",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": rate.name, "rate": rate.rate.to_string() }),
        },
    )
    .await?;

    Ok(rate)
}

pub async fn tax_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRateId) -> Result<TaxRate> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    sqlx::query_as::<_, TaxRate>(
        "select id, tax_region_id, rate, code, name, is_default, is_combinable, created_at
         from tax_rate
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tax rate"))
}

/// A field left `None` is left alone. The region a rate belongs to is not among
/// them: a rate that moved region would answer for goods it never taxed.
#[derive(Debug, Clone, Default)]
pub struct TaxRatePatch {
    pub rate: Option<Decimal>,
    pub code: Option<String>,
    pub name: Option<String>,
    pub is_default: Option<bool>,
    pub is_combinable: Option<bool>,
}

/// Changes the rate in place, keeping the id. Orders already taxed keep the
/// tax lines they were taxed with — those are copied onto the order rather than
/// joined to this row — so history does not move when the rate does.
pub async fn update_tax_rate(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: TaxRateId,
    patch: TaxRatePatch,
) -> Result<TaxRate> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a tax rate needs a name"));
    }
    if patch
        .rate
        .is_some_and(|rate| rate.is_sign_negative() || rate > Decimal::from(100))
    {
        return Err(Error::invalid(
            "a tax rate is a percentage between 0 and 100",
        ));
    }

    let rate = sqlx::query_as::<_, TaxRate>(
        "update tax_rate set
             rate = coalesce($3::numeric, rate),
             code = coalesce($4::text, code),
             name = coalesce($5::text, name),
             is_default = coalesce($6::boolean, is_default),
             is_combinable = coalesce($7::boolean, is_combinable)
         where scope = $1 and id = $2
         returning id, tax_region_id, rate, code, name, is_default, is_combinable, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.rate)
    .bind(patch.code.as_deref())
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.is_default)
    .bind(patch.is_combinable)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tax rate"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_rate",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": rate.name, "rate": rate.rate.to_string() }),
        },
    )
    .await?;

    Ok(rate)
}

pub async fn tax_rates(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    region: Option<TaxRegionId>,
    paging: Paging,
) -> Result<Page<TaxRate>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let rows = sqlx::query_as::<_, TaxRate>(
        "select id, tax_region_id, rate, code, name, is_default, is_combinable, created_at
         from tax_rate
         where scope = $1
           and ($2::uuid is null or tax_region_id = $2)
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(region.map(TaxRegionId::as_uuid))
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

pub async fn delete_tax_rate(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: TaxRateId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Tax)?;

    let done = sqlx::query("delete from tax_rate where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("tax rate"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "tax_rate",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

pub async fn create_tax_rate_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewTaxRateRule,
) -> Result<TaxRateRuleRow> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    let id = Uuid::now_v7();
    let rule = sqlx::query_as::<_, TaxRateRuleRow>(
        "insert into tax_rate_rule (id, scope, tax_rate_id, reference, reference_id)
         values ($1, $2, $3, $4, $5)
         returning id, tax_rate_id, reference, reference_id",
    )
    .bind(id)
    .bind(ctx.scope.0)
    .bind(new.tax_rate_id.as_uuid())
    .bind(new.reference.as_str())
    .bind(new.reference_id)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_rate_rule",
            entity_id: id,
            summary: serde_json::json!({ "reference": rule.reference }),
        },
    )
    .await?;

    Ok(rule)
}

pub async fn tax_rate_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    rate: TaxRateId,
) -> Result<Vec<TaxRateRuleRow>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let rows = sqlx::query_as::<_, TaxRateRuleRow>(
        "select id, tax_rate_id, reference, reference_id
         from tax_rate_rule
         where scope = $1 and tax_rate_id = $2
         order by id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(rate.as_uuid())
    .bind(MAX_TAX_RATE_RULES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn delete_tax_rate_rule(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: Uuid) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Tax)?;

    let done = sqlx::query("delete from tax_rate_rule where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id)
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("tax rate rule"));
    }

    Ok(())
}

/// The rates that answer for an address, most specific region first.
///
/// A province region and the country above it are both loaded: the province
/// answers where it has something to say, and the country is what a combinable
/// province rate stacks on top of.
pub async fn rates_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address: &TaxableAddress,
) -> Result<Vec<(TaxRegion, Vec<TaxRate>)>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let country = country_code(&address.country_code)?;
    let regions = sqlx::query_as::<_, TaxRegion>(
        "select id, country_code, province_code, parent_id, provider, created_at
         from tax_region
         where scope = $1
           and country_code = $2
           and (
             province_code is null
             or ($3::text is not null and lower(province_code) = lower($3))
           )
         -- Most specific first: a province before the country holding it.
         order by province_code nulls last
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(&country)
    .bind(address.province_code.as_deref())
    .bind(MAX_TAX_REGIONS)
    .fetch_all(&mut **tx)
    .await?;

    if regions.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<Uuid> = regions.iter().map(|region| region.id.as_uuid()).collect();
    let rates = sqlx::query_as::<_, TaxRate>(
        "select id, tax_region_id, rate, code, name, is_default, is_combinable, created_at
         from tax_rate
         where scope = $1 and tax_region_id = any($2)
         order by created_at, id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(&ids)
    .bind(MAX_TAX_RATES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(regions
        .into_iter()
        .map(|region| {
            let mine = rates
                .iter()
                .filter(|rate| rate.tax_region_id == region.id)
                .cloned()
                .collect();
            (region, mine)
        })
        .collect())
}

/// Works out the tax on each line from the rates configured here.
///
/// `is_tax_inclusive` says what the amounts already contain. Inclusive means
/// the tax is taken out of the amount rather than added to it, so a line priced
/// at 118 with an eighteen percent rate carries 18 of tax and not 21.24.
///
/// One tax line per line handed in per applicable rate: bounded by the caller's
/// input, and by [`MAX_TAX_RATES`] behind it.
pub async fn calculate(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lines: &[TaxableLine],
    address: &TaxableAddress,
    is_tax_inclusive: bool,
) -> Result<Vec<TaxLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let Some(first) = lines.first() else {
        return Ok(Vec::new());
    };

    let currency = first.amount.currency;
    if lines.iter().any(|line| line.amount.currency != currency) {
        return Err(Error::bug("tax was asked for two currencies at once"));
    }

    let chain = rates_for(tx, ctx, address).await?;
    if chain.is_empty() {
        return Ok(Vec::new());
    }

    let exponent = u32::try_from(store::currency(tx, ctx, currency).await?.exponent)
        .map_err(|_| Error::bug("a currency's exponent is not a count of decimal places"))?;

    let rate_ids: Vec<Uuid> = chain
        .iter()
        .flat_map(|(_, rates)| rates.iter().map(|rate| rate.id.as_uuid()))
        .collect();

    let rules = sqlx::query_as::<_, TaxRateRuleRow>(
        "select id, tax_rate_id, reference, reference_id
         from tax_rate_rule
         where scope = $1 and tax_rate_id = any($2)
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(&rate_ids)
    .bind(MAX_TAX_RATE_RULES)
    .fetch_all(&mut **tx)
    .await?;

    let mut out = Vec::new();
    for line in lines {
        let applicable = applicable_rates(&chain, &rules, line);
        out.extend(amounts_for(line, &applicable, is_tax_inclusive, exponent)?);
    }

    Ok(out)
}

/// Puts what [`calculate`] worked out on the cart, replacing whatever was
/// there.
///
/// A cart's tax lines are a snapshot of one answer, so they are rewritten
/// whole: a leftover line from the last address is what makes a total wrong.
/// `items` are keyed by `cart_line_item.id` and `shipping` by
/// `cart_shipping_method.id`; a line for anything else is not this cart's and
/// lands nowhere.
pub async fn set_cart_tax_lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    items: &[TaxLine],
    shipping: &[TaxLine],
) -> Result<()> {
    #[derive(FromRow)]
    struct CartRow {
        customer_id: Option<Uuid>,
        completed_at: Option<chrono::DateTime<chrono::Utc>>,
    }

    let cart = sqlx::query_as::<_, CartRow>(
        "select customer_id, completed_at from cart where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("cart"))?;

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

    sqlx::query(
        "delete from cart_line_item_tax_line t
         using cart_line_item l
         where t.scope = $1
           and l.scope = $1
           and t.cart_line_item_id = l.id
           and l.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    sqlx::query(
        "delete from cart_shipping_method_tax_line t
         using cart_shipping_method m
         where t.scope = $1
           and m.scope = $1
           and t.cart_shipping_method_id = m.id
           and m.cart_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .execute(&mut **tx)
    .await?;

    for line in items {
        sqlx::query(
            "insert into cart_line_item_tax_line (id, scope, cart_line_item_id, rate, code, name)
             select $1, $2, l.id, $4, $5, $6
             from cart_line_item l
             where l.scope = $2 and l.id = $3 and l.cart_id = $7
             on conflict (scope, cart_line_item_id, code)
             do update set rate = excluded.rate, name = excluded.name",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(line.line_id)
        .bind(line.rate)
        .bind(&line.code)
        .bind(&line.name)
        .bind(cart_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    for line in shipping {
        sqlx::query(
            "insert into cart_shipping_method_tax_line
                 (id, scope, cart_shipping_method_id, rate, code, name)
             select $1, $2, m.id, $4, $5, $6
             from cart_shipping_method m
             where m.scope = $2 and m.id = $3 and m.cart_id = $7
             on conflict (scope, cart_shipping_method_id, code)
             do update set rate = excluded.rate, name = excluded.name",
        )
        .bind(Uuid::now_v7())
        .bind(ctx.scope.0)
        .bind(line.line_id)
        .bind(line.rate)
        .bind(&line.code)
        .bind(&line.name)
        .bind(cart_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// The same question asked of somebody else's engine, for a shop whose tax is
/// not tezgah's to work out.
pub async fn calculate_with(
    ctx: &Ctx<'_>,
    provider: &dyn TaxProvider,
    lines: &[TaxableLine],
    address: &TaxableAddress,
    is_tax_inclusive: bool,
) -> Result<Vec<TaxLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    provider.tax_lines(lines, address, is_tax_inclusive).await
}

/// The most specific region with something to say answers; a combinable answer
/// stacks on the default beneath it rather than replacing it.
fn applicable_rates(
    chain: &[(TaxRegion, Vec<TaxRate>)],
    rules: &[TaxRateRuleRow],
    line: &TaxableLine,
) -> Vec<TaxRate> {
    let matches = |rate: &TaxRate| {
        rules.iter().any(|rule| {
            rule.tax_rate_id == rate.id
                && line.targets.iter().any(|target| {
                    target.id == rule.reference_id
                        && TaxReference::parse(&rule.reference)
                            .is_ok_and(|reference| reference == target.reference)
                })
        })
    };

    let mut chosen: Vec<TaxRate> = Vec::new();
    for (_, rates) in chain {
        let hits: Vec<TaxRate> = rates.iter().filter(|rate| matches(rate)).cloned().collect();
        if !hits.is_empty() {
            chosen = hits;
            break;
        }
    }

    let stacks = !chosen.is_empty() && chosen.iter().all(|rate| rate.is_combinable);
    if chosen.is_empty() || stacks {
        for (_, rates) in chain {
            if let Some(default) = rates.iter().find(|rate| rate.is_default) {
                if !chosen.iter().any(|rate| rate.id == default.id) {
                    chosen.push(default.clone());
                }
                break;
            }
        }
    }

    chosen
}

fn amounts_for(
    line: &TaxableLine,
    rates: &[TaxRate],
    is_tax_inclusive: bool,
    exponent: u32,
) -> Result<Vec<TaxLine>> {
    if rates.is_empty() {
        return Ok(Vec::new());
    }

    let hundred = Decimal::from(100);
    let currency = line.amount.currency;

    let amounts: Vec<Money> = if is_tax_inclusive {
        let total: Decimal = rates.iter().map(|rate| rate.rate).sum();
        if total.is_zero() {
            rates
                .iter()
                .map(|_| Money::new(Decimal::ZERO, currency))
                .collect()
        } else {
            let net = line.amount.amount / (Decimal::ONE + total / hundred);
            let tax = (line.amount.amount - net).round_dp(exponent);
            let weights: Vec<Decimal> = rates.iter().map(|rate| rate.rate).collect();
            allocate(Money::new(tax, currency), &weights, exponent)?
        }
    } else {
        rates
            .iter()
            .map(|rate| {
                let tax = (line.amount.amount * rate.rate / hundred).round_dp(exponent);
                Money::new(tax, currency)
            })
            .collect()
    };

    Ok(rates
        .iter()
        .zip(amounts)
        .map(|(rate, amount)| TaxLine {
            line_id: line.id,
            tax_rate_id: Some(rate.id),
            code: rate.code.clone().unwrap_or_else(|| rate.name.clone()),
            name: rate.name.clone(),
            rate: rate.rate,
            amount,
            is_tax_inclusive,
        })
        .collect())
}

fn country_code(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(Error::invalid(format!("{text:?} is not a country code")));
    }
    Ok(trimmed.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::money::Currency;

    fn currency() -> Currency {
        Currency::parse("TRY").expect("a currency code")
    }

    fn rate(percent: Decimal, combinable: bool) -> TaxRate {
        TaxRate {
            id: TaxRateId::new(),
            tax_region_id: TaxRegionId::new(),
            rate: percent,
            code: Some("vat".into()),
            name: "VAT".into(),
            is_default: true,
            is_combinable: combinable,
            created_at: chrono::Utc::now(),
        }
    }

    fn line(amount: Decimal) -> TaxableLine {
        TaxableLine {
            id: Uuid::now_v7(),
            amount: Money::new(amount, currency()),
            targets: Vec::new(),
        }
    }

    #[test]
    fn tax_is_added_on_top_when_the_price_excludes_it() {
        let lines = amounts_for(&line(dec!(100)), &[rate(dec!(18), false)], false, 2)
            .expect("an exclusive line");
        assert_eq!(lines[0].amount.amount, dec!(18.00));
    }

    #[test]
    fn tax_comes_out_of_the_price_when_it_includes_it() {
        let lines = amounts_for(&line(dec!(118)), &[rate(dec!(18), false)], true, 2)
            .expect("an inclusive line");
        assert_eq!(lines[0].amount.amount, dec!(18.00));
    }

    #[test]
    fn stacked_inclusive_rates_add_back_up_to_what_was_taken_out() {
        let rates = [rate(dec!(18), true), rate(dec!(2), true)];
        let lines = amounts_for(&line(dec!(120)), &rates, true, 2).expect("inclusive lines");
        let total: Decimal = lines.iter().map(|line| line.amount.amount).sum();
        assert_eq!(total, dec!(20.00));
    }
}
