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
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{PublishableKeyId, RegionId, SalesChannelId, StoreId};
use crate::money::Currency;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};

/// Most languages one shop is served in.
const MAX_LOCALES: i32 = 100;

/// Most currencies one shop trades in.
const MAX_SUPPORTED_CURRENCIES: i32 = 100;

/// Most channels one storefront key is allowed to see. Configuration rather
/// than anybody's data, and a key that saw half its channels would serve half
/// a shop, so this is capped rather than paged.
const MAX_KEY_CHANNELS: i64 = 200;

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

    /// How many decimal places this currency rounds to.
    ///
    /// The database keeps `exponent` between 0 and 6, so the error arm is
    /// unreachable rather than a fallback anybody should rely on.
    pub fn places(&self) -> Result<u32> {
        u32::try_from(self.exponent)
            .map_err(|_| Error::bug("a currency's exponent is not a count of decimal places"))
    }

    /// Rounds to this currency's smallest unit. Anything handed to a provider
    /// goes through here first.
    pub fn round(&self, amount: Decimal) -> Result<Decimal> {
        Ok(amount.round_dp(self.places()?))
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
    let _: Permit = ctx.permit(Action::View, Resource::Store)?;

    read_store(tx, ctx).await
}

pub async fn update_store(tx: &mut Tx<'_>, ctx: &Ctx<'_>, patch: StorePatch) -> Result<Store> {
    let _: Permit = ctx.permit(Action::Write, Resource::Store)?;

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

/// The currencies this shop prices in. Configuration rather than anybody's
/// data, so it is capped rather than paged.
pub async fn currencies(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<CurrencyRow>> {
    let _: Permit = ctx.permit(Action::View, Resource::Channel { id: None })?;

    let rows = sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency
         where scope = $1 order by code limit $2",
    )
    .bind(ctx.scope.0)
    .bind(MAX_SUPPORTED_CURRENCIES)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn currency(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: Currency) -> Result<CurrencyRow> {
    let _: Permit = ctx.permit(Action::View, Resource::Channel { id: None })?;

    sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency where scope = $1 and code = $2",
    )
    .bind(ctx.scope.0)
    .bind(code.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("currency"))
}

/// How many decimal places a currency rounds to, for everything that rounds.
///
/// One reader rather than three: a shop selling in a currency it has not
/// configured is a `not_found` rather than two decimal places, which is the
/// wrong answer for JPY and for KWD and silently the right one for neither.
pub async fn exponent(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: Currency) -> Result<u32> {
    currency(tx, ctx, code).await?.places()
}

pub async fn create_region(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewRegion) -> Result<Region> {
    let _: Permit = ctx.permit(Action::Write, Resource::Channel { id: None })?;

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
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Channel {
            id: Some(id.as_uuid()),
        },
    )?;

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

/// A field left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct RegionPatch {
    pub name: Option<String>,
    pub currency_code: Option<Currency>,
    pub is_tax_inclusive: Option<bool>,
}

pub async fn update_region(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: RegionId,
    patch: RegionPatch,
) -> Result<Region> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Channel {
            id: Some(id.as_uuid()),
        },
    )?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a region needs a name"));
    }

    let region = sqlx::query_as::<_, Region>(
        "update region set
             name = coalesce($3::text, name),
             currency_code = coalesce($4::text, currency_code),
             is_tax_inclusive = coalesce($5::boolean, is_tax_inclusive)
         where scope = $1 and id = $2
         returning id, name, currency_code, is_tax_inclusive, created_at",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.currency_code.map(|c| c.as_str().to_owned()))
    .bind(patch.is_tax_inclusive)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("region"))?;

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

/// The languages this shop writes in. Configuration rather than anybody's data,
/// so it is capped rather than paged.
pub async fn locales(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<String>> {
    let _: Permit = ctx.permit(Action::View, Resource::Store)?;

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
    let _: Permit = ctx.permit(Action::View, Resource::Channel { id: None })?;

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

/// One ISO 3166-1 country, and the region it is served from.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct RegionCountry {
    pub id: Uuid,
    pub iso_2: String,
    pub iso_3: String,
    pub numeric_code: String,
    pub name: String,
    pub display_name: String,
    pub region_id: Option<RegionId>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewRegionCountry {
    pub iso_2: String,
    pub iso_3: String,
    pub numeric_code: String,
    pub name: String,
    pub display_name: Option<String>,
}

const REGION_COUNTRY_COLUMNS: &str =
    "id, iso_2, iso_3, numeric_code, name, display_name, region_id, created_at";

fn iso_2(text: &str) -> Result<String> {
    let trimmed = text.trim();
    if trimmed.len() != 2 || !trimmed.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(Error::invalid("a country code is two letters"));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Puts a country under a region, moving it if it was under another one: a
/// country is served from one place, and two regions claiming it is two
/// currencies for one address.
pub async fn add_region_country(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    region_id: RegionId,
    new: NewRegionCountry,
) -> Result<RegionCountry> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Channel {
            id: Some(region_id.as_uuid()),
        },
    )?;

    let code = iso_2(&new.iso_2)?;
    let iso_3 = new.iso_3.trim().to_ascii_uppercase();
    if iso_3.len() != 3 || !iso_3.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(Error::invalid("an alpha-3 country code is three letters"));
    }
    let numeric = new.numeric_code.trim().to_string();
    if numeric.len() != 3 || !numeric.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::invalid("a numeric country code is three digits"));
    }
    let name = new.name.trim().to_string();
    if name.is_empty() {
        return Err(Error::invalid("a country needs a name"));
    }
    let display = new
        .display_name
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| name.clone());

    let row = sqlx::query_as::<_, RegionCountry>(&format!(
        "insert into region_country
             (id, scope, iso_2, iso_3, numeric_code, name, display_name, region_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         on conflict (scope, iso_2) do update set
             iso_3 = excluded.iso_3,
             numeric_code = excluded.numeric_code,
             name = excluded.name,
             display_name = excluded.display_name,
             region_id = excluded.region_id
         returning {REGION_COUNTRY_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(&code)
    .bind(&iso_3)
    .bind(&numeric)
    .bind(&name)
    .bind(&display)
    .bind(region_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "region_country",
            entity_id: row.id,
            summary: serde_json::json!({ "iso_2": row.iso_2, "region_id": region_id.as_uuid() }),
        },
    )
    .await?;

    Ok(row)
}

/// Takes a country out of its region without deleting it: ISO 3166-1 is a
/// fixed list, and the row is the list rather than the shop's decision.
pub async fn remove_region_country(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, Resource::Channel { id: None })?;

    let code = iso_2(code)?;
    let done = sqlx::query(
        "update region_country set region_id = null
         where scope = $1 and iso_2 = $2 and region_id is not null",
    )
    .bind(ctx.scope.0)
    .bind(&code)
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("region country"));
    }

    Ok(())
}

pub async fn region_countries(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    region_id: RegionId,
    paging: Paging,
) -> Result<Page<RegionCountry>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Channel {
            id: Some(region_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, RegionCountry>(&format!(
        "select {REGION_COUNTRY_COLUMNS}
         from region_country
         where scope = $1 and region_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(region_id.as_uuid())
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

/// Which region serves a delivery country, and `None` while nobody has said.
///
/// `None` is not an error: a shop that has never filled the table in is every
/// shop running today, and the answer there is that the country decides
/// nothing rather than that the sale is refused.
pub async fn region_for_country(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    code: &str,
) -> Result<Option<Region>> {
    let _: Permit = ctx.permit(Action::View, Resource::Channel { id: None })?;

    let code = iso_2(code)?;
    Ok(sqlx::query_as::<_, Region>(
        "select r.id, r.name, r.currency_code, r.is_tax_inclusive, r.created_at
         from region_country c
         join region r on r.scope = c.scope and r.id = c.region_id
         where c.scope = $1 and c.iso_2 = $2",
    )
    .bind(ctx.scope.0)
    .bind(&code)
    .fetch_optional(&mut **tx)
    .await?)
}

#[derive(Debug, Clone)]
pub struct NewSalesChannel {
    pub name: String,
    pub description: Option<String>,
    pub is_disabled: bool,
}

/// A field left `None` is left alone.
#[derive(Debug, Clone, Default)]
pub struct SalesChannelPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_disabled: Option<bool>,
}

const CHANNEL_COLUMNS: &str = "id, name, description, is_disabled, created_at";

pub async fn create_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewSalesChannel,
) -> Result<SalesChannel> {
    let _: Permit = ctx.permit(Action::Write, Resource::Channel { id: None })?;

    if new.name.trim().is_empty() {
        return Err(Error::invalid("a sales channel needs a name"));
    }

    let id = SalesChannelId::new();
    let channel = sqlx::query_as::<_, SalesChannel>(&format!(
        "insert into sales_channel (id, scope, name, description, is_disabled)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, name) do nothing
         returning {CHANNEL_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.name.trim())
    .bind(new.description.as_deref())
    .bind(new.is_disabled)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::conflict("a channel of that name is already here"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "sales_channel",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": channel.name }),
        },
    )
    .await?;

    Ok(channel)
}

pub async fn update_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
    patch: SalesChannelPatch,
) -> Result<SalesChannel> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Channel {
            id: Some(id.as_uuid()),
        },
    )?;

    if patch
        .name
        .as_ref()
        .is_some_and(|name| name.trim().is_empty())
    {
        return Err(Error::invalid("a sales channel needs a name"));
    }

    let channel = sqlx::query_as::<_, SalesChannel>(&format!(
        "update sales_channel set
             name = coalesce($3::text, name),
             description = coalesce($4::text, description),
             is_disabled = coalesce($5::boolean, is_disabled)
         where scope = $1 and id = $2
         returning {CHANNEL_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(patch.name.as_deref().map(str::trim))
    .bind(patch.description.as_deref())
    .bind(patch.is_disabled)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("sales channel"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "sales_channel",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "name": channel.name }),
        },
    )
    .await?;

    Ok(channel)
}

pub async fn delete_sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::Channel {
            id: Some(id.as_uuid()),
        },
    )?;

    let done = sqlx::query("delete from sales_channel where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("sales channel"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "sales_channel",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

pub async fn sales_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: SalesChannelId,
) -> Result<SalesChannel> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Channel {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, SalesChannel>(&format!(
        "select {CHANNEL_COLUMNS} from sales_channel where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("sales channel"))
}

pub async fn sales_channels(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<SalesChannel>> {
    let _: Permit = ctx.permit(Action::View, Resource::Channel { id: None })?;

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

// -------------------------------------------------------- publishable keys

/// What a storefront sends instead of an admin credential. It carries no
/// permission of its own; it says which channels the request may see.
///
/// The token is not here and cannot be read back: only its hash is stored.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PublishableKey {
    pub id: PublishableKeyId,
    pub title: String,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl PublishableKey {
    pub fn is_live(&self) -> bool {
        self.revoked_at.is_none()
    }
}

/// A fresh key and the one time its token is readable. Whoever asked has to
/// put it in the storefront now or ask for another.
#[derive(Debug, Clone)]
pub struct IssuedKey {
    pub key: PublishableKey,
    pub token: String,
}

const KEY_COLUMNS: &str = "id, title, revoked_at, last_used_at, created_at";

pub async fn create_publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    title: &str,
) -> Result<IssuedKey> {
    let _: Permit = ctx.permit(Action::Write, Resource::PublishableKey { id: None })?;

    if title.trim().is_empty() {
        return Err(Error::invalid("a publishable key needs a title"));
    }

    let token = fresh_token(tx).await?;
    let id = PublishableKeyId::new();
    let key = sqlx::query_as::<_, PublishableKey>(&format!(
        "insert into publishable_key (id, scope, title, token)
         values ($1, $2, $3, $4)
         returning {KEY_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(title.trim())
    .bind(digest(&token))
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "publishable_key",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "title": key.title }),
        },
    )
    .await?;

    Ok(IssuedKey { key, token })
}

/// Revoking is a state rather than a delete: the storefront that was using it
/// is still worth knowing about, and the links to its channels stay.
pub async fn revoke_publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
) -> Result<PublishableKey> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::PublishableKey {
            id: Some(id.as_uuid()),
        },
    )?;

    let key = sqlx::query_as::<_, PublishableKey>(&format!(
        "update publishable_key set revoked_at = $3
         where scope = $1 and id = $2 and revoked_at is null
         returning {KEY_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .fetch_optional(&mut **tx)
    .await?;

    let Some(key) = key else {
        return Err(match publishable_key(tx, ctx, id).await {
            Ok(_) => Error::conflict("that key was already revoked"),
            Err(error) => error,
        });
    };

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "publishable_key",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "revoked": true }),
        },
    )
    .await?;

    Ok(key)
}

pub async fn publishable_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PublishableKeyId,
) -> Result<PublishableKey> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::PublishableKey {
            id: Some(id.as_uuid()),
        },
    )?;

    sqlx::query_as::<_, PublishableKey>(&format!(
        "select {KEY_COLUMNS} from publishable_key where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("publishable key"))
}

pub async fn publishable_keys(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<PublishableKey>> {
    let _: Permit = ctx.permit(Action::View, Resource::PublishableKey { id: None })?;

    let rows = sqlx::query_as::<_, PublishableKey>(&format!(
        "select {KEY_COLUMNS} from publishable_key
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
        id: row.id.as_uuid(),
    }))
}

pub async fn link_key_to_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    key: PublishableKeyId,
    channel: SalesChannelId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::PublishableKey {
            id: Some(key.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "insert into publishable_key_sales_channel
             (id, scope, publishable_key_id, sales_channel_id)
         select $1::uuid, $2::uuid, k.id, c.id
         from publishable_key k
         join sales_channel c on c.scope = k.scope and c.id = $4
         where k.scope = $2 and k.id = $3
         on conflict (scope, publishable_key_id, sales_channel_id) do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(key.as_uuid())
    .bind(channel.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        // Nothing inserted is either a link that was already there or a key or
        // channel this scope does not have, and only the second is an error.
        let linked = channels_for_key(tx, ctx, key)
            .await?
            .into_iter()
            .any(|row| row.id == channel);
        if !linked {
            return Err(Error::not_found("publishable key"));
        }
        return Ok(());
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "publishable_key_sales_channel",
            entity_id: key.as_uuid(),
            summary: serde_json::json!({ "sales_channel_id": channel.as_uuid() }),
        },
    )
    .await?;

    Ok(())
}

pub async fn unlink_key_from_channel(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    key: PublishableKeyId,
    channel: SalesChannelId,
) -> Result<()> {
    let _: Permit = ctx.permit(
        Action::Delete,
        Resource::PublishableKey {
            id: Some(key.as_uuid()),
        },
    )?;

    let done = sqlx::query(
        "delete from publishable_key_sales_channel
         where scope = $1 and publishable_key_id = $2 and sales_channel_id = $3",
    )
    .bind(ctx.scope.0)
    .bind(key.as_uuid())
    .bind(channel.as_uuid())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("publishable key channel"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "publishable_key_sales_channel",
            entity_id: key.as_uuid(),
            summary: serde_json::json!({ "sales_channel_id": channel.as_uuid() }),
        },
    )
    .await?;

    Ok(())
}

/// The channels one key may see, capped by how many a key is allowed.
pub async fn channels_for_key(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    key: PublishableKeyId,
) -> Result<Vec<SalesChannel>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::PublishableKey {
            id: Some(key.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, SalesChannel>(
        "select c.id, c.name, c.description, c.is_disabled, c.created_at
         from publishable_key_sales_channel l
         join sales_channel c on c.scope = l.scope and c.id = l.sales_channel_id
         where l.scope = $1 and l.publishable_key_id = $2
         order by c.created_at, c.id
         limit $3",
    )
    .bind(ctx.scope.0)
    .bind(key.as_uuid())
    .bind(MAX_KEY_CHANNELS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

/// What a storefront request holding this token may see.
///
/// A token that is not this shop's, or has been revoked, is a denial rather
/// than an empty list: an empty list is a shop with no channels, and the two
/// should not answer the same.
pub async fn channels_for_token(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
) -> Result<Vec<SalesChannel>> {
    let key = key_for_token(tx, ctx, token).await?;

    channels_for_key(tx, ctx, key).await
}

/// Which live key a token is, or a denial.
async fn key_for_token(tx: &mut Tx<'_>, ctx: &Ctx<'_>, token: &str) -> Result<PublishableKeyId> {
    let _: Permit = ctx.permit(Action::View, Resource::PublishableKey { id: None })?;

    let held: Option<(Uuid, String)> = sqlx::query_as(
        "select id, token from publishable_key
         where scope = $1 and token = $2 and revoked_at is null",
    )
    .bind(ctx.scope.0)
    .bind(digest(token))
    .fetch_optional(&mut **tx)
    .await?;

    let Some((id, held)) = held else {
        return Err(Error::denied());
    };

    // Constant time: a comparison that stops at the first wrong byte tells
    // whoever is guessing how much of the token they have right.
    let matches: bool = digest(token).as_bytes().ct_eq(held.as_bytes()).into();
    if !matches {
        return Err(Error::denied());
    }

    Ok(PublishableKeyId::from_uuid(id))
}

/// 256 bits from the database's own generator, so no host has to supply one.
async fn fresh_token(tx: &mut Tx<'_>) -> Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "select 'pk_' || replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '')",
    )
    .fetch_one(&mut **tx)
    .await?)
}

fn digest(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
