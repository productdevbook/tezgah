//! Who the shop is to a tax authority, and who the buyer is.
//!
//! The rate tables answer what a jurisdiction takes. These routes answer the
//! question asked before that one: where the shop is established and under
//! which schemes it files, what number the buyer gave and whether anybody
//! checked it, and which certificate exempts them. Without them every sale is
//! a domestic sale to a consumer, whatever the buyer is.
//!
//! # This is personal data
//!
//! A tax number and a certificate identify a person or a business. They are
//! carried in views because a back office has to see what it filed, and they
//! are kept out of audit summaries, errors and logs: an error naming a number
//! puts it in every log that catches the error.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::id::CustomerId;
use crate::ports::{Action, Ctx, Tx};
use crate::tax;

use super::{Method, Route, Surface};

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// One registration the shop itself holds.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct RegistrationView {
    pub id: Uuid,
    pub country_code: String,
    pub scheme: String,
    pub tax_id: Option<String>,
    pub is_home: bool,
    pub valid_from: chrono::DateTime<chrono::Utc>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<tax::TaxRegistration> for RegistrationView {
    fn from(row: tax::TaxRegistration) -> Self {
        RegistrationView {
            id: row.id,
            country_code: row.country_code,
            scheme: row.scheme,
            tax_id: row.tax_id,
            is_home: row.is_home,
            valid_from: row.valid_from,
            valid_until: row.valid_until,
            created_at: row.created_at,
        }
    }
}

/// A buyer's number. `validated_at` is what decides anything: an unchecked
/// number is a string somebody typed.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TaxIdView {
    pub id: Uuid,
    pub customer_id: CustomerId,
    pub tax_id: String,
    pub tax_id_type: String,
    pub tax_id_country: String,
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub evidence: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<tax::CustomerTaxId> for TaxIdView {
    fn from(row: tax::CustomerTaxId) -> Self {
        TaxIdView {
            id: row.id,
            customer_id: row.customer_id,
            tax_id: row.tax_id,
            tax_id_type: row.tax_id_type,
            tax_id_country: row.tax_id_country,
            validated_at: row.validated_at,
            evidence: row.evidence,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExemptionView {
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
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<tax::TaxExemption> for ExemptionView {
    fn from(row: tax::TaxExemption) -> Self {
        ExemptionView {
            id: row.id,
            customer_id: row.customer_id,
            kind: row.kind,
            reason_code: row.reason_code,
            certificate_reference: row.certificate_reference,
            country_code: row.country_code,
            province_code: row.province_code,
            valid_from: row.valid_from,
            valid_until: row.valid_until,
            verified_at: row.verified_at,
            created_at: row.created_at,
        }
    }
}

// ---------------------------------------------------------------------------
// The shop's own registrations
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegisterShop {
    pub country_code: String,
    /// `domestic`, `oss`, `ioss`, `other`.
    pub scheme: String,
    pub tax_id: Option<String>,
    pub is_home: bool,
    pub valid_from: Option<chrono::DateTime<chrono::Utc>>,
    pub valid_until: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn register_shop(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: RegisterShop,
) -> Result<RegistrationView> {
    Ok(RegistrationView::from(
        tax::register_shop(
            tx,
            ctx,
            tax::NewTaxRegistration {
                country_code: body.country_code,
                scheme: body.scheme,
                tax_id: body.tax_id,
                is_home: body.is_home,
                valid_from: body.valid_from,
                valid_until: body.valid_until,
            },
        )
        .await?,
    ))
}

/// Capped rather than paged in the domain: which schemes the shop files under
/// decides how a distance sale is labelled, and a page would relabel it.
pub async fn list_registrations(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<RegistrationView>> {
    Ok(tax::registrations(tx, ctx)
        .await?
        .into_iter()
        .map(RegistrationView::from)
        .collect())
}

pub async fn delete_registration(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: Uuid) -> Result<()> {
    tax::delete_registration(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// A buyer's numbers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordTaxId {
    pub tax_id: String,
    pub tax_id_type: String,
    pub tax_id_country: String,
    /// When the host checked the number with the authority. Absent means
    /// nobody did, and an unchecked number moves nothing.
    pub validated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// What that check returned — a consultation number, kept because it is
    /// proof of what was known on the day of the sale.
    pub evidence: Option<String>,
}

pub async fn record_tax_id(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    body: RecordTaxId,
) -> Result<TaxIdView> {
    Ok(TaxIdView::from(
        tax::record_tax_id(
            tx,
            ctx,
            tax::NewCustomerTaxId {
                customer_id,
                tax_id: body.tax_id,
                tax_id_type: body.tax_id_type,
                tax_id_country: body.tax_id_country,
                validated_at: body.validated_at,
                evidence: body.evidence,
            },
        )
        .await?,
    ))
}

/// Capped in the domain by `MAX_TAX_IDS`: the number left off the end of a
/// page is the one the reverse charge rested on.
pub async fn list_tax_ids(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<Vec<TaxIdView>> {
    Ok(tax::tax_ids(tx, ctx, customer_id)
        .await?
        .into_iter()
        .map(TaxIdView::from)
        .collect())
}

pub async fn delete_tax_id(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: Uuid) -> Result<()> {
    tax::delete_tax_id(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Certificates
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantExemption {
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

pub async fn grant_exemption(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    body: GrantExemption,
) -> Result<ExemptionView> {
    Ok(ExemptionView::from(
        tax::grant_exemption(
            tx,
            ctx,
            tax::NewTaxExemption {
                customer_id,
                kind: body.kind,
                reason_code: body.reason_code,
                certificate_reference: body.certificate_reference,
                country_code: body.country_code,
                province_code: body.province_code,
                valid_from: body.valid_from,
                valid_until: body.valid_until,
                verified_at: body.verified_at,
                evidence: body.evidence,
            },
        )
        .await?,
    ))
}

/// Capped in the domain by `MAX_TAX_EXEMPTIONS`, expired certificates
/// included: an order placed last year has to stay explainable.
pub async fn list_exemptions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
) -> Result<Vec<ExemptionView>> {
    Ok(tax::exemptions(tx, ctx, customer_id)
        .await?
        .into_iter()
        .map(ExemptionView::from)
        .collect())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeExemption {
    /// When it stops applying. Absent is now.
    pub at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Ends a certificate rather than deleting it: the orders it justified still
/// point at it.
pub async fn revoke_exemption(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Uuid,
    body: RevokeExemption,
) -> Result<ExemptionView> {
    Ok(ExemptionView::from(
        tax::revoke_exemption(tx, ctx, id, body.at).await?,
    ))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/tax-registrations",
        action: Action::View,
        domain: "tax",
        query: None,
        summary: "List where the shop is registered and what it files under",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/tax-registrations",
        action: Action::Write,
        domain: "tax",
        query: None,
        summary: "Record where the shop is registered",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/tax-registrations/{id}",
        action: Action::Delete,
        domain: "tax",
        query: None,
        summary: "Take a registration off the shop",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}/tax-ids",
        action: Action::View,
        domain: "tax",
        query: None,
        summary: "List the tax numbers a customer has given",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers/{id}/tax-ids",
        action: Action::Write,
        domain: "tax",
        query: None,
        summary: "Record a customer's tax number and what checking it returned",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/tax-ids/{id}",
        action: Action::Delete,
        domain: "tax",
        query: None,
        summary: "Forget a customer's tax number",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}/tax-exemptions",
        action: Action::View,
        domain: "tax",
        query: None,
        summary: "List the certificates a customer holds",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers/{id}/tax-exemptions",
        action: Action::Write,
        domain: "tax",
        query: None,
        summary: "File a certificate that exempts a customer",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/tax-exemptions/{id}/revoke",
        action: Action::Write,
        domain: "tax",
        query: None,
        summary: "Stop a certificate applying from a given moment",
    },
];
