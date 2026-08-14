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

use std::collections::HashMap;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{TaxRateId, TaxRegionId};
use crate::money::{Currency, Money, allocate};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};
use crate::store;

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
         order by id",
    )
    .bind(ctx.scope.0)
    .bind(rate.as_uuid())
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
         order by province_code nulls last",
    )
    .bind(ctx.scope.0)
    .bind(&country)
    .bind(address.province_code.as_deref())
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
         order by created_at, id",
    )
    .bind(ctx.scope.0)
    .bind(&ids)
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
pub async fn calculate(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    lines: &[TaxableLine],
    address: &TaxableAddress,
    is_tax_inclusive: bool,
) -> Result<Vec<TaxLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let chain = rates_for(tx, ctx, address).await?;
    if chain.is_empty() {
        return Ok(Vec::new());
    }

    let rate_ids: Vec<Uuid> = chain
        .iter()
        .flat_map(|(_, rates)| rates.iter().map(|rate| rate.id.as_uuid()))
        .collect();

    let rules = sqlx::query_as::<_, TaxRateRuleRow>(
        "select id, tax_rate_id, reference, reference_id
         from tax_rate_rule
         where scope = $1 and tax_rate_id = any($2)",
    )
    .bind(ctx.scope.0)
    .bind(&rate_ids)
    .fetch_all(&mut **tx)
    .await?;

    let mut exponents: HashMap<Currency, u32> = HashMap::new();
    for line in lines {
        if let std::collections::hash_map::Entry::Vacant(slot) =
            exponents.entry(line.amount.currency)
        {
            let row = store::currency(tx, ctx, line.amount.currency).await?;
            slot.insert(u32::try_from(row.exponent).unwrap_or(2));
        }
    }

    let mut out = Vec::new();
    for line in lines {
        let applicable = applicable_rates(&chain, &rules, line);
        let exponent = exponents.get(&line.amount.currency).copied().unwrap_or(2);
        out.extend(amounts_for(line, &applicable, is_tax_inclusive, exponent)?);
    }

    Ok(out)
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
    use super::*;
    use rust_decimal_macros::dec;

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
