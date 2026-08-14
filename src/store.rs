//! The shop itself: its currencies, its regions, and the channels it sells
//! through.
//!
//! This module is the shape every other domain follows. A call takes the
//! transaction first and the context second, asks for a [`Permit`] before it
//! touches a row, and writes its audit row and its events inside the caller's
//! transaction so a rollback takes them too.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::error::{Error, Result};
use crate::id::{RegionId, SalesChannelId};
use crate::money::Currency;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};

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

pub async fn currencies(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<CurrencyRow>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency order by code",
    )
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn currency(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: Currency) -> Result<CurrencyRow> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, CurrencyRow>(
        "select code, symbol, name, exponent from currency where code = $1",
    )
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

pub async fn regions(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<Region>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, Region>(
        "select id, name, currency_code, is_tax_inclusive, created_at
         from region
         where ($1::timestamptz is null or (created_at, id) > ($1, $2))
         order by created_at, id
         limit $3",
    )
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
         where ($1::timestamptz is null or (created_at, id) > ($1, $2))
         order by created_at, id
         limit $3",
    )
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
