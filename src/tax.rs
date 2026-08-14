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
use crate::id::{CartId, CustomerId, TaxRateId, TaxRegionId};
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

/// A buyer's numbers and certificates decide whether a rate applies at all, so
/// they are read whole rather than paged: a page would charge a buyer who is
/// exempt on the certificate that fell off the end.
const MAX_TAX_IDS: i64 = 50;
const MAX_TAX_EXEMPTIONS: i64 = 100;
const MAX_TAX_REGISTRATIONS: i64 = 200;

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
#[derive(Debug, Clone, Default)]
pub struct TaxableAddress {
    pub country_code: String,
    pub province_code: Option<String>,
    pub postal_code: Option<String>,
}

impl TaxableAddress {
    pub fn to(country_code: impl Into<String>) -> Self {
        TaxableAddress {
            country_code: country_code.into(),
            province_code: None,
            postal_code: None,
        }
    }
}

/// Why a line was taxed the way it was. `zero` and `exempt` and
/// `reverse_charge` all put nothing on the invoice and mean three different
/// things to three different authorities, so the reason is stored beside the
/// number rather than inferred from it later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum TaxTreatment {
    #[default]
    Standard,
    Zero,
    Exempt,
    ReverseCharge,
    Oss,
    Ioss,
    OutOfScope,
}

impl TaxTreatment {
    pub fn as_str(self) -> &'static str {
        match self {
            TaxTreatment::Standard => "standard",
            TaxTreatment::Zero => "zero",
            TaxTreatment::Exempt => "exempt",
            TaxTreatment::ReverseCharge => "reverse_charge",
            TaxTreatment::Oss => "oss",
            TaxTreatment::Ioss => "ioss",
            TaxTreatment::OutOfScope => "out_of_scope",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "standard" => Ok(TaxTreatment::Standard),
            "zero" => Ok(TaxTreatment::Zero),
            "exempt" => Ok(TaxTreatment::Exempt),
            "reverse_charge" => Ok(TaxTreatment::ReverseCharge),
            "oss" => Ok(TaxTreatment::Oss),
            "ioss" => Ok(TaxTreatment::Ioss),
            "out_of_scope" => Ok(TaxTreatment::OutOfScope),
            other => Err(Error::invalid(format!("{other:?} is not a tax treatment"))),
        }
    }

    /// Whether the line carries money for the authority at all.
    pub fn is_charged(self) -> bool {
        matches!(
            self,
            TaxTreatment::Standard | TaxTreatment::Zero | TaxTreatment::Oss | TaxTreatment::Ioss
        )
    }
}

/// Which authority a line's share belongs to. A United States sale is a state
/// line, a county line and a city line, each remitted separately, so the level
/// travels with the amount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxJurisdiction {
    /// `country`, `state`, `province`, `county`, `city`, `district`, `special`.
    pub level: String,
    pub code: String,
    pub name: Option<String>,
}

/// One of the two non-contradictory pieces a distance sale is placed by: where
/// the address said, where the card was issued, where the address resolved to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxEvidence {
    pub source: String,
    pub country_code: String,
}

/// A number the buyer gave, and what was known about it when it was checked.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CustomerTaxId {
    pub id: Uuid,
    pub customer_id: CustomerId,
    pub tax_id: String,
    pub tax_id_type: String,
    pub tax_id_country: String,
    /// `None` means nobody has checked it. A number nobody checked does not
    /// move the reverse charge.
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// What the check returned that day — a VIES consultation number is proof
    /// of what was known then, and asking again next year proves nothing about
    /// the sale that already happened.
    pub evidence: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewCustomerTaxId {
    pub customer_id: CustomerId,
    pub tax_id: String,
    pub tax_id_type: String,
    pub tax_id_country: String,
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaxExemption {
    pub id: Uuid,
    pub customer_id: CustomerId,
    pub kind: String,
    pub reason_code: Option<String>,
    pub certificate_reference: Option<String>,
    pub country_code: String,
    pub province_code: Option<String>,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub evidence: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTaxExemption {
    pub customer_id: CustomerId,
    pub kind: String,
    pub reason_code: Option<String>,
    pub certificate_reference: Option<String>,
    pub country_code: String,
    pub province_code: Option<String>,
    pub valid_from: Option<chrono::DateTime<chrono::Utc>>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub verified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub evidence: Option<String>,
}

/// One registration the shop itself holds.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TaxRegistration {
    pub id: Uuid,
    pub country_code: String,
    /// `domestic`, `oss`, `ioss`, `other`.
    pub scheme: String,
    pub tax_id: Option<String>,
    /// The one country the shop is established in. There is at most one.
    pub is_home: bool,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTaxRegistration {
    pub country_code: String,
    pub scheme: String,
    pub tax_id: Option<String>,
    pub is_home: bool,
    pub valid_from: Option<chrono::DateTime<chrono::Utc>>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
}

/// Who is buying, as far as tax cares — gathered once and handed to
/// [`calculate`], so the decision is made from what was true at that moment
/// rather than from what the customer row says next year.
#[derive(Debug, Clone, Default)]
pub struct TaxSubject {
    pub customer_id: Option<CustomerId>,
    /// A sale to a business is the only kind the reverse charge reaches.
    pub is_business: bool,
    pub tax_ids: Vec<CustomerTaxId>,
    pub exemptions: Vec<TaxExemption>,
    /// Where the shop is established, and under which schemes it files.
    pub registrations: Vec<TaxRegistration>,
    /// The non-contradictory pieces that placed the buyer, for a distance sale.
    pub evidence: Vec<TaxEvidence>,
}

impl TaxSubject {
    fn home_country(&self) -> Option<&str> {
        self.registrations
            .iter()
            .find(|row| row.is_home)
            .map(|row| row.country_code.as_str())
    }

    fn files_under(&self, scheme: &str) -> bool {
        self.registrations.iter().any(|row| row.scheme == scheme)
    }

    /// A checked number issued by the country the goods are going to. An
    /// unchecked one is a string the buyer typed.
    fn validated_id_for(&self, country: &str) -> Option<&CustomerTaxId> {
        self.tax_ids.iter().find(|row| {
            row.validated_at.is_some() && row.tax_id_country.eq_ignore_ascii_case(country)
        })
    }

    /// A certificate that covers this address and has not run out.
    fn exemption_for(
        &self,
        address: &TaxableAddress,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Option<&TaxExemption> {
        self.exemptions.iter().find(|row| {
            row.country_code.eq_ignore_ascii_case(&address.country_code)
                && row.valid_from <= at
                && row.valid_until.is_none_or(|until| until > at)
                && match (&row.province_code, &address.province_code) {
                    (None, _) => true,
                    (Some(mine), Some(theirs)) => mine.eq_ignore_ascii_case(theirs),
                    (Some(_), None) => false,
                }
        })
    }
}

/// What [`calculate`] decided before any arithmetic: the treatment, and the
/// paper it rests on.
#[derive(Debug, Clone, Default)]
pub struct TaxDecision {
    pub treatment: TaxTreatment,
    pub tax_id: Option<String>,
    pub tax_id_evidence: Option<String>,
    pub exemption_id: Option<Uuid>,
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
    /// The variant's taxability code, or the product's where the variant has
    /// none. It is an input to whoever calculates, and it belongs to the shop.
    pub tax_code: Option<String>,
    /// Where this one line is supplied, when that is not where the rest of the
    /// cart is going. An audiobook in a parcel's cart is taxed where the buyer
    /// is, not where the parcel lands.
    pub address: Option<TaxableAddress>,
}

/// One authority's share of one line, and everything an audit will ask about
/// it. Written once onto the cart or the order and never recomputed: an OSS
/// record is kept for ten years, and the rate table it came from will not
/// still say what it said.
#[derive(Debug, Clone)]
pub struct TaxLine {
    pub line_id: Uuid,
    pub tax_rate_id: Option<TaxRateId>,
    pub code: String,
    pub name: String,
    pub rate: Decimal,
    pub amount: Money,
    pub is_tax_inclusive: bool,
    pub treatment: TaxTreatment,
    pub jurisdiction: Option<TaxJurisdiction>,
    /// The taxability code the product was priced under.
    pub tax_code: Option<String>,
    pub provider: Option<String>,
    /// What the provider called this calculation. A refund quotes it rather
    /// than asking the same question again.
    pub provider_transaction_id: Option<String>,
    pub calculated_at: chrono::DateTime<chrono::Utc>,
    /// The address the answer was worked out from, not the one on the customer
    /// row today.
    pub address: TaxableAddress,
    pub tax_id: Option<String>,
    pub tax_id_evidence: Option<String>,
    pub exemption_id: Option<Uuid>,
    pub evidence: Vec<TaxEvidence>,
}

impl TaxLine {
    /// A line that charges nothing, and says why.
    pub fn nil(
        line_id: Uuid,
        currency: crate::money::Currency,
        treatment: TaxTreatment,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Self {
        TaxLine {
            line_id,
            tax_rate_id: None,
            code: treatment.as_str().to_string(),
            name: treatment.as_str().to_string(),
            rate: Decimal::ZERO,
            amount: Money::new(Decimal::ZERO, currency),
            is_tax_inclusive: false,
            treatment,
            jurisdiction: None,
            tax_code: None,
            provider: None,
            provider_transaction_id: None,
            calculated_at: at,
            address: TaxableAddress::default(),
            tax_id: None,
            tax_id_evidence: None,
            exemption_id: None,
            evidence: Vec::new(),
        }
    }
}

/// What an outside engine answered: its lines and the name it filed them under.
#[derive(Debug, Clone, Default)]
pub struct TaxQuote {
    /// The provider's own id for this calculation. A credit note has to name
    /// the transaction it reverses, so this is carried onto the order rather
    /// than thrown away with the response.
    pub transaction_id: Option<String>,
    pub lines: Vec<TaxLine>,
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
        subject: Option<&TaxSubject>,
        is_tax_inclusive: bool,
    ) -> Result<TaxQuote>;

    /// Reverses part or all of an earlier calculation. Not the same question
    /// asked again with negative amounts: an authority wants the credit tied to
    /// the document it credits, so the original `transaction_id` goes back.
    ///
    /// The default refuses, so a provider that cannot do it says so rather than
    /// quietly filing an unrelated document.
    async fn refund(
        &self,
        _original_transaction_id: &str,
        _lines: &[TaxableLine],
        _address: &TaxableAddress,
    ) -> Result<TaxQuote> {
        Err(Error::invalid(
            "this tax provider cannot reverse a transaction",
        ))
    }
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
    subject: Option<&TaxSubject>,
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

    let at = ctx.now();

    let mut places: Vec<Place> = Vec::new();
    for line in lines {
        let here = line.address.as_ref().unwrap_or(address);
        if places.iter().any(|place| place.is(here)) {
            continue;
        }
        places.push(place(tx, ctx, here, subject, at).await?);
    }

    let exponent = if places.iter().any(|place| !place.chain.is_empty()) {
        u32::try_from(store::currency(tx, ctx, currency).await?.exponent)
            .map_err(|_| Error::bug("a currency's exponent is not a count of decimal places"))?
    } else {
        0
    };

    let mut out = Vec::new();
    for line in lines {
        let here = line.address.as_ref().unwrap_or(address);
        let place = places
            .iter()
            .find(|place| place.is(here))
            .ok_or_else(|| Error::bug("a line was taxed at an address nobody looked up"))?;
        let decision = &place.decision;
        let evidence = subject.map(|s| s.evidence.clone()).unwrap_or_default();

        // Nothing is charged, so no rate is used: a rate table that changed
        // since cannot change what this line says.
        if !decision.treatment.is_charged() {
            out.push(TaxLine {
                is_tax_inclusive,
                tax_code: line.tax_code.clone(),
                address: here.clone(),
                tax_id: decision.tax_id.clone(),
                tax_id_evidence: decision.tax_id_evidence.clone(),
                exemption_id: decision.exemption_id,
                evidence,
                ..TaxLine::nil(line.id, currency, decision.treatment, at)
            });
            continue;
        }

        let applicable = applicable_rates(&place.chain, &place.rules, line);
        let mut priced = amounts_for(line, &applicable, is_tax_inclusive, exponent)?;
        for row in &mut priced {
            row.treatment = decision.treatment;
            row.tax_code = line.tax_code.clone();
            row.calculated_at = at;
            row.address = here.clone();
            row.tax_id = decision.tax_id.clone();
            row.tax_id_evidence = decision.tax_id_evidence.clone();
            row.evidence = evidence.clone();
        }
        out.extend(priced);
    }

    Ok(out)
}

/// One address's answer, worked out once however many lines are supplied there.
struct Place {
    address: TaxableAddress,
    decision: TaxDecision,
    chain: Vec<(TaxRegion, Vec<TaxRate>)>,
    rules: Vec<TaxRateRuleRow>,
}

impl Place {
    fn is(&self, address: &TaxableAddress) -> bool {
        self.address
            .country_code
            .eq_ignore_ascii_case(&address.country_code)
            && self.address.province_code == address.province_code
            && self.address.postal_code == address.postal_code
    }
}

async fn place(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    address: &TaxableAddress,
    subject: Option<&TaxSubject>,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<Place> {
    let decision = decide(address, subject, at)?;

    let chain = if decision.treatment.is_charged() {
        rates_for(tx, ctx, address).await?
    } else {
        Vec::new()
    };

    let rate_ids: Vec<Uuid> = chain
        .iter()
        .flat_map(|(_, rates)| rates.iter().map(|rate| rate.id.as_uuid()))
        .collect();

    let rules = if rate_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as::<_, TaxRateRuleRow>(
            "select id, tax_rate_id, reference, reference_id
             from tax_rate_rule
             where scope = $1 and tax_rate_id = any($2)
             limit $3",
        )
        .bind(ctx.scope.0)
        .bind(&rate_ids)
        .bind(MAX_TAX_RATE_RULES)
        .fetch_all(&mut **tx)
        .await?
    };

    Ok(Place {
        address: address.clone(),
        decision,
        chain,
        rules,
    })
}

/// Whether this buyer is charged at all, decided before a rate is looked up.
///
/// Order matters. A certificate the buyer holds beats everything, because a
/// reseller in Texas is exempt whatever the rate says. Then the reverse charge:
/// a checked VAT number issued where the goods are going, a business buyer, and
/// a seller established somewhere else — all three, because two of them is a
/// domestic sale and the seller still charges.
///
/// Which countries are in which union is not decided here and is not in the
/// schema: the shop's own registrations say where it files, and the operator
/// keeps those current. Embedding the list would date the library.
fn decide(
    address: &TaxableAddress,
    subject: Option<&TaxSubject>,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<TaxDecision> {
    let Some(subject) = subject else {
        return Ok(TaxDecision::default());
    };

    if let Some(exemption) = subject.exemption_for(address, at) {
        return Ok(TaxDecision {
            treatment: TaxTreatment::Exempt,
            exemption_id: Some(exemption.id),
            ..TaxDecision::default()
        });
    }

    let home = subject.home_country();
    let crosses = home.is_some_and(|home| !home.eq_ignore_ascii_case(&address.country_code));

    if subject.is_business && crosses {
        if let Some(number) = subject.validated_id_for(&address.country_code) {
            return Ok(TaxDecision {
                treatment: TaxTreatment::ReverseCharge,
                tax_id: Some(number.tax_id.clone()),
                tax_id_evidence: number.evidence.clone(),
                ..TaxDecision::default()
            });
        }
    }

    // A consumer abroad is still charged — at their country's rate — but the
    // return it lands on is the scheme's, not the home country's.
    if crosses && !subject.is_business {
        let scheme = if subject.files_under("oss") {
            Some(TaxTreatment::Oss)
        } else if subject.files_under("ioss") {
            Some(TaxTreatment::Ioss)
        } else {
            None
        };

        if let Some(treatment) = scheme {
            places_the_buyer(&subject.evidence, address)?;
            return Ok(TaxDecision {
                treatment,
                ..TaxDecision::default()
            });
        }
    }

    Ok(TaxDecision::default())
}

/// Whether the pieces on file put the buyer where the sale says they are.
///
/// Two, from different sources, agreeing with each other and with the address:
/// one piece is only the address saying itself again, and two that disagree
/// place the buyer nowhere. A piece naming no country is not a piece.
fn places_the_buyer(evidence: &[TaxEvidence], address: &TaxableAddress) -> Result<()> {
    let mut sources: Vec<&str> = Vec::new();
    for piece in evidence {
        if country_code(&piece.country_code).is_err() {
            continue;
        }
        if !piece
            .country_code
            .trim()
            .eq_ignore_ascii_case(&address.country_code)
        {
            return Err(Error::invalid(
                "the evidence for where this buyer is contradicts itself",
            ));
        }
        let source = piece.source.trim();
        if !source.is_empty() && !sources.iter().any(|seen| seen.eq_ignore_ascii_case(source)) {
            sources.push(source);
        }
    }

    if sources.len() < 2 {
        return Err(Error::invalid(
            "a distance sale to a consumer abroad needs two pieces of evidence for where they are",
        ));
    }

    Ok(())
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

    for (rows, table, column, parent) in [
        (
            items,
            "cart_line_item_tax_line",
            "cart_line_item_id",
            "cart_line_item",
        ),
        (
            shipping,
            "cart_shipping_method_tax_line",
            "cart_shipping_method_id",
            "cart_shipping_method",
        ),
    ] {
        for line in rows {
            sqlx::query(&format!(
                "insert into {table}
                     (id, scope, {column}, rate, code, name, treatment, jurisdiction_level,
                      jurisdiction_code, jurisdiction_name, tax_code, provider,
                      provider_transaction_id, calculated_at, address_country_code,
                      address_province_code, address_postal_code, tax_id, tax_id_evidence,
                      exemption_id, evidence)
                 select $1, $2, p.id, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                        $15, $16, $17, $18, $19, $20, $21
                 from {parent} p
                 where p.scope = $2 and p.id = $3 and p.cart_id = $22
                 on conflict (scope, {column}, code, coalesce(jurisdiction_code, ''))
                 do update set
                     rate = excluded.rate,
                     name = excluded.name,
                     treatment = excluded.treatment,
                     jurisdiction_level = excluded.jurisdiction_level,
                     jurisdiction_name = excluded.jurisdiction_name,
                     tax_code = excluded.tax_code,
                     provider = excluded.provider,
                     provider_transaction_id = excluded.provider_transaction_id,
                     calculated_at = excluded.calculated_at,
                     address_country_code = excluded.address_country_code,
                     address_province_code = excluded.address_province_code,
                     address_postal_code = excluded.address_postal_code,
                     tax_id = excluded.tax_id,
                     tax_id_evidence = excluded.tax_id_evidence,
                     exemption_id = excluded.exemption_id,
                     evidence = excluded.evidence"
            ))
            .bind(Uuid::now_v7())
            .bind(ctx.scope.0)
            .bind(line.line_id)
            .bind(line.rate)
            .bind(&line.code)
            .bind(&line.name)
            .bind(line.treatment.as_str())
            .bind(line.jurisdiction.as_ref().map(|j| j.level.as_str()))
            .bind(line.jurisdiction.as_ref().map(|j| j.code.as_str()))
            .bind(line.jurisdiction.as_ref().and_then(|j| j.name.as_deref()))
            .bind(line.tax_code.as_deref())
            .bind(line.provider.as_deref())
            .bind(line.provider_transaction_id.as_deref())
            .bind(line.calculated_at)
            .bind(country_or_null(&line.address.country_code))
            .bind(line.address.province_code.as_deref())
            .bind(line.address.postal_code.as_deref())
            .bind(line.tax_id.as_deref())
            .bind(line.tax_id_evidence.as_deref())
            .bind(line.exemption_id)
            .bind(evidence_json(&line.evidence))
            .bind(cart_id.as_uuid())
            .execute(&mut **tx)
            .await?;
        }
    }

    Ok(())
}

fn country_or_null(code: &str) -> Option<String> {
    country_code(code).ok()
}

fn evidence_json(evidence: &[TaxEvidence]) -> Option<serde_json::Value> {
    if evidence.is_empty() {
        return None;
    }
    serde_json::to_value(evidence).ok()
}

/// The same question asked of somebody else's engine, for a shop whose tax is
/// not tezgah's to work out.
/// The provider's own transaction id and the address asked about are stamped
/// onto every line it hands back, so a provider that forgot to fill them in
/// still leaves an order that can be reconciled and refunded.
pub async fn calculate_with(
    ctx: &Ctx<'_>,
    provider: &dyn TaxProvider,
    lines: &[TaxableLine],
    address: &TaxableAddress,
    subject: Option<&TaxSubject>,
    is_tax_inclusive: bool,
) -> Result<Vec<TaxLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    let quote = provider
        .tax_lines(lines, address, subject, is_tax_inclusive)
        .await?;

    Ok(stamp(quote, provider.code(), address, ctx.now()))
}

/// Reverses a calculation somebody else made, naming the document it credits.
pub async fn refund_with(
    ctx: &Ctx<'_>,
    provider: &dyn TaxProvider,
    original_transaction_id: &str,
    lines: &[TaxableLine],
    address: &TaxableAddress,
) -> Result<Vec<TaxLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    if original_transaction_id.trim().is_empty() {
        return Err(Error::invalid(
            "a tax refund has to name the transaction it reverses",
        ));
    }

    let quote = provider
        .refund(original_transaction_id, lines, address)
        .await?;

    Ok(stamp(quote, provider.code(), address, ctx.now()))
}

fn stamp(
    quote: TaxQuote,
    code: &str,
    address: &TaxableAddress,
    at: chrono::DateTime<chrono::Utc>,
) -> Vec<TaxLine> {
    quote
        .lines
        .into_iter()
        .map(|mut line| {
            line.provider.get_or_insert_with(|| code.to_string());
            if line.provider_transaction_id.is_none() {
                line.provider_transaction_id = quote.transaction_id.clone();
            }
            if line.address.country_code.is_empty() {
                line.address = address.clone();
            }
            line.calculated_at = at;
            line
        })
        .collect()
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
            treatment: TaxTreatment::Standard,
            jurisdiction: None,
            tax_code: line.tax_code.clone(),
            provider: None,
            provider_transaction_id: None,
            calculated_at: chrono::Utc::now(),
            address: TaxableAddress::default(),
            tax_id: None,
            tax_id_evidence: None,
            exemption_id: None,
            evidence: Vec::new(),
        })
        .collect())
}

const TAX_ID_COLUMNS: &str = "id, customer_id, tax_id, tax_id_type, tax_id_country,
     validated_at, evidence, created_at";

const EXEMPTION_COLUMNS: &str = "id, customer_id, kind, reason_code, certificate_reference,
     country_code, province_code, valid_from, valid_until, verified_at, evidence, created_at";

const REGISTRATION_COLUMNS: &str =
    "id, country_code, scheme, tax_id, is_home, valid_from, valid_until, created_at";

/// Records a number the buyer gave and, where the host has checked it, the
/// answer it got. Recording it again for the same country replaces it: the
/// evidence that matters is the copy frozen on the order, not this row.
pub async fn record_tax_id(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewCustomerTaxId,
) -> Result<CustomerTaxId> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(new.customer_id.as_uuid()),
        },
    )?;

    let number = new.tax_id.trim();
    if number.is_empty() {
        return Err(Error::invalid("a tax id needs a number"));
    }
    let country = country_code(&new.tax_id_country)?;

    let row = sqlx::query_as::<_, CustomerTaxId>(&format!(
        "insert into customer_tax_id
             (id, scope, customer_id, tax_id, tax_id_type, tax_id_country, validated_at, evidence)
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         on conflict (scope, customer_id, tax_id_country, tax_id) do update set
             tax_id_type = excluded.tax_id_type,
             validated_at = excluded.validated_at,
             evidence = excluded.evidence
         returning {TAX_ID_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.customer_id.as_uuid())
    .bind(number)
    .bind(new.tax_id_type.trim())
    .bind(&country)
    .bind(new.validated_at)
    .bind(new.evidence.as_deref())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "customer_tax_id",
            entity_id: row.id,
            summary: serde_json::json!({
                "tax_id_country": row.tax_id_country,
                "validated": row.validated_at.is_some(),
            }),
        },
    )
    .await?;

    Ok(row)
}

/// Capped by [`MAX_TAX_IDS`]: a buyer's numbers are wanted whole, because the
/// one left off the end is the one the reverse charge rested on.
pub async fn tax_ids(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<Vec<CustomerTaxId>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    Ok(sqlx::query_as::<_, CustomerTaxId>(&format!(
        "select {TAX_ID_COLUMNS}
         from customer_tax_id
         where scope = $1 and customer_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(MAX_TAX_IDS)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn delete_tax_id(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: Uuid) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Customer { id: None })?;

    let done = sqlx::query("delete from customer_tax_id where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id)
        .execute(&mut **tx)
        .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("customer tax id"));
    }

    Ok(())
}

/// Files a certificate the buyer holds. Its dates are kept as given; whether it
/// still applies is asked at calculation time, so a certificate that ran out
/// yesterday stops exempting today without anybody having to sweep the table.
pub async fn grant_exemption(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewTaxExemption,
) -> Result<TaxExemption> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: Some(new.customer_id.as_uuid()),
        },
    )?;

    let country = country_code(&new.country_code)?;
    let from = new.valid_from.unwrap_or_else(|| ctx.now());
    if new.valid_until.is_some_and(|until| until <= from) {
        return Err(Error::invalid("an exemption expires after it starts"));
    }

    let row = sqlx::query_as::<_, TaxExemption>(&format!(
        "insert into tax_exemption
             (id, scope, customer_id, kind, reason_code, certificate_reference, country_code,
              province_code, valid_from, valid_until, verified_at, evidence)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
         returning {EXEMPTION_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(new.customer_id.as_uuid())
    .bind(new.kind.trim())
    .bind(new.reason_code.as_deref())
    .bind(new.certificate_reference.as_deref())
    .bind(&country)
    .bind(new.province_code.as_deref().map(str::trim))
    .bind(from)
    .bind(new.valid_until)
    .bind(new.verified_at)
    .bind(new.evidence.as_deref())
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_exemption",
            entity_id: row.id,
            summary: serde_json::json!({ "kind": row.kind, "country_code": row.country_code }),
        },
    )
    .await?;

    Ok(row)
}

/// Every certificate on file for a buyer, expired ones included: an order
/// placed last year has to be explainable by a certificate that has since run
/// out. Capped by [`MAX_TAX_EXEMPTIONS`].
pub async fn exemptions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<Vec<TaxExemption>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    Ok(sqlx::query_as::<_, TaxExemption>(&format!(
        "select {EXEMPTION_COLUMNS}
         from tax_exemption
         where scope = $1 and customer_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(MAX_TAX_EXEMPTIONS)
    .fetch_all(&mut **tx)
    .await?)
}

/// Ends a certificate rather than deleting it: the orders it justified still
/// point at it, and a row that vanished is a zero nobody can explain.
pub async fn revoke_exemption(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Uuid,
    at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<TaxExemption> {
    let _: Permit = ctx.permit(Action::Write, Resource::Customer { id: None })?;

    let row = sqlx::query_as::<_, TaxExemption>(&format!(
        "update tax_exemption set valid_until = $3
         where scope = $1 and id = $2
         returning {EXEMPTION_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id)
    .bind(at.unwrap_or_else(|| ctx.now()))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("tax exemption"))?;

    Ok(row)
}

/// Records where the shop itself is registered. `is_home` marks the one country
/// it is established in, and setting it clears whoever held it, because the
/// database allows only one.
pub async fn register_shop(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewTaxRegistration,
) -> Result<TaxRegistration> {
    let _: Permit = ctx.permit(Action::Write, Resource::Tax)?;

    let country = country_code(&new.country_code)?;

    if new.is_home {
        sqlx::query("update tax_registration set is_home = false where scope = $1 and is_home")
            .bind(ctx.scope.0)
            .execute(&mut **tx)
            .await?;
    }

    let row = sqlx::query_as::<_, TaxRegistration>(&format!(
        "insert into tax_registration
             (id, scope, country_code, scheme, tax_id, is_home, valid_from, valid_until)
         values ($1, $2, $3, $4, $5, $6, coalesce($7, now()), $8)
         on conflict (scope, country_code, scheme) do update set
             tax_id = excluded.tax_id,
             is_home = excluded.is_home,
             valid_from = excluded.valid_from,
             valid_until = excluded.valid_until
         returning {REGISTRATION_COLUMNS}"
    ))
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(&country)
    .bind(new.scheme.trim())
    .bind(new.tax_id.as_deref())
    .bind(new.is_home)
    .bind(new.valid_from)
    .bind(new.valid_until)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "tax_registration",
            entity_id: row.id,
            summary: serde_json::json!({ "country_code": row.country_code, "scheme": row.scheme }),
        },
    )
    .await?;

    Ok(row)
}

/// Capped by [`MAX_TAX_REGISTRATIONS`]: which schemes the shop files under
/// decides how a distance sale is labelled, and a page would relabel it.
pub async fn registrations(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<TaxRegistration>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    Ok(sqlx::query_as::<_, TaxRegistration>(&format!(
        "select {REGISTRATION_COLUMNS}
         from tax_registration
         where scope = $1
         order by created_at, id
         limit $2"
    ))
    .bind(ctx.scope.0)
    .bind(MAX_TAX_REGISTRATIONS)
    .fetch_all(&mut **tx)
    .await?)
}

/// The taxability code each of these variants is sold under, the product's
/// standing in where the variant names none.
///
/// Bounded by what the caller hands in, and by [`crate::cart::MAX_LINES`]
/// behind that: the only callers are pricing one cart or one order.
pub async fn tax_codes(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_ids: &[Uuid],
) -> Result<std::collections::HashMap<Uuid, String>> {
    let _: Permit = ctx.permit(Action::View, Resource::Tax)?;

    if variant_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    #[derive(FromRow)]
    struct Row {
        id: Uuid,
        tax_code: Option<String>,
    }

    let rows = sqlx::query_as::<_, Row>(
        "select v.id, coalesce(v.tax_code, p.tax_code) as tax_code
         from product_variant v
         left join product p on p.scope = v.scope and p.id = v.product_id
         where v.scope = $1 and v.id = any($2)",
    )
    .bind(ctx.scope.0)
    .bind(variant_ids)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows
        .into_iter()
        .filter_map(|row| row.tax_code.map(|code| (row.id, code)))
        .collect())
}

/// Gathers what is known about a buyer into the shape [`calculate`] decides
/// from. Read once at the moment of the sale, so the answer is the state of the
/// world then and not whatever the customer edits afterwards.
///
/// A guest with no customer row still gets the shop's own registrations, which
/// is what places the sale.
pub async fn subject_for(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: Option<CustomerId>,
    is_business: bool,
    evidence: Vec<TaxEvidence>,
) -> Result<TaxSubject> {
    let registrations = registrations(tx, ctx).await?;

    let (tax_ids, exemptions) = match customer_id {
        Some(id) => (
            self::tax_ids(tx, ctx, id).await?,
            self::exemptions(tx, ctx, id).await?,
        ),
        None => (Vec::new(), Vec::new()),
    };

    Ok(TaxSubject {
        customer_id,
        is_business,
        tax_ids,
        exemptions,
        registrations,
        evidence,
    })
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
            tax_code: None,
            address: None,
        }
    }

    fn subject(home: &str, business: bool) -> TaxSubject {
        TaxSubject {
            is_business: business,
            registrations: vec![TaxRegistration {
                id: Uuid::now_v7(),
                country_code: home.into(),
                scheme: "domestic".into(),
                tax_id: None,
                is_home: true,
                valid_from: chrono::Utc::now(),
                valid_until: None,
                created_at: chrono::Utc::now(),
            }],
            ..TaxSubject::default()
        }
    }

    fn evidence(source: &str, country: &str) -> TaxEvidence {
        TaxEvidence {
            source: source.into(),
            country_code: country.into(),
        }
    }

    /// A shop at home in Turkey that files a German consumer's VAT under OSS.
    fn filing_under_oss() -> TaxSubject {
        let mut buyer = subject("TR", false);
        buyer.registrations.push(TaxRegistration {
            id: Uuid::now_v7(),
            country_code: "DE".into(),
            scheme: "oss".into(),
            tax_id: None,
            is_home: false,
            valid_from: chrono::Utc::now(),
            valid_until: None,
            created_at: chrono::Utc::now(),
        });
        buyer
    }

    fn validated(country: &str) -> CustomerTaxId {
        CustomerTaxId {
            id: Uuid::now_v7(),
            customer_id: CustomerId::new(),
            tax_id: format!("{country}123456789"),
            tax_id_type: "vat".into(),
            tax_id_country: country.into(),
            validated_at: Some(chrono::Utc::now()),
            evidence: Some("WAPIAAAA-consultation".into()),
            created_at: chrono::Utc::now(),
        }
    }

    fn certificate(country: &str, until: Option<chrono::DateTime<chrono::Utc>>) -> TaxExemption {
        TaxExemption {
            id: Uuid::now_v7(),
            customer_id: CustomerId::new(),
            kind: "resale_certificate".into(),
            reason_code: None,
            certificate_reference: Some("ST-120".into()),
            country_code: country.into(),
            province_code: None,
            valid_from: chrono::Utc::now() - chrono::Duration::days(30),
            valid_until: until,
            verified_at: None,
            evidence: None,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_cross_border_business_with_a_checked_number_accounts_for_its_own_vat() {
        let mut buyer = subject("TR", true);
        buyer.tax_ids = vec![validated("DE")];

        let decided = decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect("a decision");

        assert_eq!(decided.treatment, TaxTreatment::ReverseCharge);
        assert!(!decided.treatment.is_charged());
        assert_eq!(
            decided.tax_id_evidence.as_deref(),
            Some("WAPIAAAA-consultation"),
            "the consultation number is what proves what was known that day"
        );
    }

    #[test]
    fn a_number_nobody_checked_does_not_shift_the_charge() {
        let mut buyer = subject("TR", true);
        let mut unchecked = validated("DE");
        unchecked.validated_at = None;
        unchecked.evidence = None;
        buyer.tax_ids = vec![unchecked];

        let decided = decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect("a decision");
        assert_eq!(decided.treatment, TaxTreatment::Standard);
    }

    #[test]
    fn a_business_at_home_is_charged_however_good_its_number_is() {
        let mut buyer = subject("DE", true);
        buyer.tax_ids = vec![validated("DE")];

        let decided = decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect("a decision");
        assert_eq!(decided.treatment, TaxTreatment::Standard);
    }

    #[test]
    fn a_certificate_exempts_until_the_day_it_runs_out() {
        let now = chrono::Utc::now();
        let mut buyer = subject("US", true);
        buyer.exemptions = vec![certificate("US", Some(now + chrono::Duration::days(1)))];
        assert_eq!(
            decide(&TaxableAddress::to("US"), Some(&buyer), now)
                .expect("a decision")
                .treatment,
            TaxTreatment::Exempt
        );

        buyer.exemptions = vec![certificate("US", Some(now - chrono::Duration::days(1)))];
        assert_eq!(
            decide(&TaxableAddress::to("US"), Some(&buyer), now)
                .expect("a decision")
                .treatment,
            TaxTreatment::Standard
        );
    }

    #[test]
    fn a_consumer_abroad_is_charged_but_filed_under_the_scheme() {
        let mut buyer = subject("TR", false);
        buyer.registrations.push(TaxRegistration {
            id: Uuid::now_v7(),
            country_code: "DE".into(),
            scheme: "oss".into(),
            tax_id: None,
            is_home: false,
            valid_from: chrono::Utc::now(),
            valid_until: None,
            created_at: chrono::Utc::now(),
        });
        buyer.evidence = vec![
            evidence("billing_address", "DE"),
            evidence("card_issuer", "DE"),
        ];

        let decided = decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect("a decision");
        assert_eq!(decided.treatment, TaxTreatment::Oss);
        assert!(
            decided.treatment.is_charged(),
            "OSS charges the buyer's rate; it only changes which return it lands on"
        );
    }

    #[test]
    fn one_piece_of_evidence_does_not_place_a_consumer_abroad() {
        let mut buyer = filing_under_oss();
        buyer.evidence = vec![evidence("billing_address", "DE")];

        decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect_err("a sale placed by the address saying itself again");
    }

    #[test]
    fn two_pieces_that_disagree_place_the_consumer_nowhere() {
        let mut buyer = filing_under_oss();
        buyer.evidence = vec![
            evidence("billing_address", "DE"),
            evidence("card_issuer", "FR"),
        ];

        decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect_err("a sale placed by contradicting evidence");
    }

    #[test]
    fn the_same_source_twice_is_one_piece() {
        let mut buyer = filing_under_oss();
        buyer.evidence = vec![
            evidence("billing_address", "DE"),
            evidence("billing_address", "DE"),
        ];

        decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect_err("one source repeated");
    }

    #[test]
    fn a_sale_at_home_needs_no_evidence_at_all() {
        let buyer = subject("DE", false);
        let decided = decide(&TaxableAddress::to("DE"), Some(&buyer), chrono::Utc::now())
            .expect("a domestic sale");
        assert_eq!(decided.treatment, TaxTreatment::Standard);
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
