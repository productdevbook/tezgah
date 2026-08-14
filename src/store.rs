//! The shop itself: its currencies, its regions, and the channels it sells
//! through.
//!
//! This module is the shape every other domain follows. A call takes the
//! transaction first and the context second, asks for a [`Permit`] before it
//! touches a row, and writes its audit row and its events inside the caller's
//! transaction so a rollback takes them too.
//!
//! Every scoped query names its scope even though a policy already filters by
//! it. The policy is the guarantee; the predicate is what still holds when a
//! host connects as an owner or a superuser, which bypasses policies
//! altogether. It costs nothing — the index starts with `scope` — and it means
//! a misconfigured deployment reads nothing rather than everything.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{RegionId, SalesChannelId, StoreId};
use crate::money::Currency;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};

/// Most languages one shop is served in.
const MAX_LOCALES: i32 = 100;

/// Most currencies one shop trades in.
const MAX_SUPPORTED_CURRENCIES: i32 = 100;

/// A currency the shop trades in, and how many decimal places it rounds to.
///
/// The exponent lives here and nowhere else: it is what every allocation and
/// every provider amount is rounded by, and a second copy of it is a second
/// answer.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CurrencyRow {
    pub code: String,
    pub symbol: String,
    pub name: String,
    /// 0 for JPY, 2 for TRY, 3 for KWD.
    pub exponent: i16,
}

impl CurrencyRow {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.code)
    }

    /// Rounds to this currency's smallest unit. Anything handed to a provider
    /// goes through here first.
    pub fn round(&self, amount: Decimal) -> Decimal {
        amount.round_dp(u32::try_from(self.exponent).unwrap_or(2))
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Region {
    pub id: RegionId,
    pub name: String,
    pub currency_code: String,
    /// Whether the prices shown in this region already contain tax.
    pub is_tax_inclusive: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewRegion {
    pub name: String,
    pub currency_code: Currency,
    pub is_tax_inclusive: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SalesChannel {
    pub id: SalesChannelId,
    pub name: String,
    pub description: Option<String>,
    pub is_disabled: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The shop itself. One row per scope, so it is read without an id.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Store {
    pub id: StoreId,
    pub name: String,
    pub default_currency_code: String,
    pub supported_currency_codes: Vec<String>,
    pub supported_locales: Vec<String>,
    pub default_region_id: Option<RegionId>,
    pub default_sales_channel_id: Option<SalesChannelId>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// A field left `None` is left alone. Nothing here can be unset back to null;
/// a default region is changed rather than removed.
#[derive(Debug, Clone, Default)]
pub struct StorePatch {
    pub name: Option<String>,
    pub default_currency_code: Option<Currency>,
    pub supported_currency_codes: Option<Vec<Currency>>,
    pub supported_locales: Option<Vec<String>>,
    pub default_region_id: Option<RegionId>,
    pub default_sales_channel_id: Option<SalesChannelId>,
    pub metadata: Option<serde_json::Value>,
}

async fn read_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Store> {
    sqlx::query_as::<_, Store>(
        "select id, name, default_currency_code::text as default_currency_code,
                supported_currency_codes[1:$2::int]::text[] as supported_currency_codes,
                supported_locales[1:$3::int] as supported_locales,
                default_region_id, default_sales_channel_id, metadata, created_at, updated_at
         from store
         where scope = $1",
    )
    .bind(ctx.scope.0)
    .bind(MAX_SUPPORTED_CURRENCIES)
    .bind(MAX_LOCALES)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("store"))
}

pub async fn store(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Store> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    read_store(tx, ctx).await
}

pub async fn update_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>, patch: StorePatch) -> Result<Store> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a shop needs a name"));
    }
    if patch
        .supported_locales
        .as_ref()
        .is_some_and(|locales| locales.len() > MAX_LOCALES as usize)
    {
        return Err(Error::invalid("that is more locales than a shop may have"));
    }
    if patch
        .supported_currency_codes
        .as_ref()
        .is_some_and(|codes| codes.len() > MAX_SUPPORTED_CURRENCIES as usize)
    {
        return Err(Error::invalid(
            "that is more currencies than a shop may have",
        ));
    }

    let currencies: Option<Vec<String>> = patch
        .supported_currency_codes
        .as_ref()
        .map(|codes| codes.iter().map(|c| c.as_str().to_owned()).collect());

    let done = sqlx::query(
        "update store set
             name = coalesce($2::text, name),
             default_currency_code = coalesce($3::text, default_currency_code),
             supported_currency_codes =
                 coalesce($4::text[], supported_currency_codes::text[])::char(3)[],
             supported_locales = coalesce($5::text[], supported_locales),
             default_region_id = coalesce($6::uuid, default_region_id),
             default_sales_channel_id = coalesce($7::uuid, default_sales_channel_id),
             metadata = coalesce($8::jsonb, metadata)
         where scope = $1",
    )
    .bind(ctx.scope.0)
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.default_currency_code.map(|c| c.as_str().to_owned()))
    .bind(currencies)
    .bind(patch.supported_locales.clone())
    .bind(patch.default_region_id.map(|id| id.as_uuid()))
    .bind(patch.default_sales_channel_id.map(|id| id.as_uuid()))
    .bind(patch.metadata.as_ref())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("store"));
    }

    let store = read_store(tx, ctx).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "store",
            entity_id: store.id.as_uuid(),
            summary: serde_json::json!({ "name": store.name }),
        },
    )
    .await?;

    Ok(store)
}

pub async fn currencies(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<CurrencyRow>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency where scope = $1 order by code",
    )
    .bind(ctx.scope.0)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn currency(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: Currency) -> Result<CurrencyRow> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency where scope = $1 and code = $2",
    )
    .bind(ctx.scope.0)
    .bind(code.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("currency"))
}

pub async fn create_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewRegion) -> Result<Region> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a region needs a name"));
    }

    let id = RegionId::new();
    let region = sqlx::query_as::<_, Region>(
        "insert into region (id, scope, name, currency_code, is_tax_inclusive)
         values ($1, $2, $3, $4, $5)
         returning id, name, currency_code, is_tax_inclusive, created_at",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(new.currency_code.as_str())
    .bind(new.is_tax_inclusive)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "region",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": region.name }),
        },
    )
    .await?;

    Ok(region)
}

pub async fn region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: RegionId) -> Result<Region> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, Region>(
        "select id, name, currency_code, is_tax_inclusive, created_at
         from region
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("region"))
}

/// The languages this shop writes in. Configuration rather than anybody's data,
/// so it is capped rather than paged.
pub async fn locales(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let found: Option<Vec<String>> = sqlx::query_scalar(
        "select supported_locales[1:$2::int] from store where scope = $1 limit 1",
    )
    .bind(ctx.scope.0)
    .bind(MAX_LOCALES)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found.unwrap_or_default())
}

pub async fn regions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<Region>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, Region>(
        "select id, name, currency_code, is_tax_inclusive, created_at
         from region
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

pub async fn sales_channels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<SalesChannel>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, SalesChannel>(
        "select id, name, description, is_disabled, created_at
         from sales_channel
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
