//! What the buyer agreed to, when the window to change their mind closes, and
//! the invoice that was issued for the order.
//!
//! Three things a distance seller has to be able to answer afterwards, and all
//! three are evidence rather than state: a version is written once and never
//! edited, an acceptance names the text it accepted by its hash, and an invoice
//! carries the serial the authority allocated. Nothing here recomputes an old
//! answer from today's rules.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::id::{AgreementVersionId, OrderAgreementId, OrderId, OrderInvoiceId, ReturnId};
use crate::money::{Currency, Money};
use crate::order;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};

use super::{Method, Route, Surface};

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgreementVersionView {
    pub id: AgreementVersionId,
    pub kind: String,
    pub locale: String,
    pub body: String,
    /// What the acceptance points at, so two copies of a text can be compared
    /// without reading them.
    pub body_hash: String,
    pub effective_from: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::AgreementVersion> for AgreementVersionView {
    fn from(row: order::AgreementVersion) -> Self {
        AgreementVersionView {
            id: row.id,
            kind: row.kind,
            locale: row.locale,
            body: row.body,
            body_hash: row.body_hash,
            effective_from: row.effective_from,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderAgreementView {
    pub id: OrderAgreementId,
    pub order_id: OrderId,
    pub agreement_version_id: AgreementVersionId,
    pub kind: String,
    pub body_hash: String,
    pub accepted_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::OrderAgreement> for OrderAgreementView {
    fn from(row: order::OrderAgreement) -> Self {
        OrderAgreementView {
            id: row.id,
            order_id: row.order_id,
            agreement_version_id: row.agreement_version_id,
            kind: row.kind,
            body_hash: row.body_hash,
            accepted_at: row.accepted_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvoiceView {
    pub id: OrderInvoiceId,
    pub order_id: OrderId,
    pub order_version: i32,
    pub kind: String,
    pub number: String,
    pub external_id: Option<String>,
    pub provider: Option<String>,
    pub status: String,
    pub issued_at: Option<chrono::DateTime<chrono::Utc>>,
    pub document_url: Option<String>,
    pub total_amount: Decimal,
    pub currency_code: String,
    pub replaces_invoice_id: Option<OrderInvoiceId>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<order::OrderInvoice> for InvoiceView {
    fn from(row: order::OrderInvoice) -> Self {
        InvoiceView {
            id: row.id,
            order_id: row.order_id,
            order_version: row.order_version,
            kind: row.kind,
            number: row.number,
            external_id: row.external_id,
            provider: row.provider,
            status: row.status,
            issued_at: row.issued_at,
            document_url: row.document_url,
            total_amount: row.total_amount,
            currency_code: row.currency_code,
            replaces_invoice_id: row.replaces_invoice_id,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalView {
    pub order_line_item_id: crate::id::LineItemId,
    pub eligible: bool,
    pub exclusion_reason: Option<String>,
    pub delivered_at: Option<chrono::DateTime<chrono::Utc>>,
    pub deadline: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<order::LineWithdrawal> for WithdrawalView {
    fn from(row: order::LineWithdrawal) -> Self {
        WithdrawalView {
            order_line_item_id: row.order_line_item_id,
            eligible: row.eligible,
            exclusion_reason: row.exclusion_reason,
            delivered_at: row.delivered_at,
            deadline: row.deadline,
        }
    }
}

// ---------------------------------------------------------------------------
// Agreements
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishAgreement {
    pub kind: String,
    pub locale: String,
    pub body: String,
    pub effective_from: Option<chrono::DateTime<chrono::Utc>>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn publish_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: PublishAgreement,
) -> Result<AgreementVersionView> {
    let new = order::NewAgreement {
        kind: order::AgreementKind::parse(&body.kind)?,
        locale: body.locale,
        body: body.body,
        effective_from: body.effective_from,
        metadata: body.metadata,
    };

    Ok(AgreementVersionView::from(
        order::publish_agreement(tx, ctx, new).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAgreements {
    pub kind: Option<String>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_agreements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListAgreements,
) -> Result<Page<AgreementVersionView>> {
    let kind = query
        .kind
        .as_deref()
        .map(order::AgreementKind::parse)
        .transpose()?;

    let page =
        order::agreement_versions(tx, ctx, kind, paging(query.after.as_deref(), query.limit)?)
            .await?;

    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(AgreementVersionView::from)
            .collect(),
        next: page.next,
    })
}

pub async fn get_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: AgreementVersionId,
) -> Result<AgreementVersionView> {
    Ok(AgreementVersionView::from(
        order::agreement_version(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptAgreement {
    pub agreement_version_id: AgreementVersionId,
    pub accepted_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Where from and with what, as the host saw it. tezgah does not read a
    /// request, so whoever does hands these over.
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn accept_agreement(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    body: AcceptAgreement,
) -> Result<OrderAgreementView> {
    let acceptance = order::Acceptance {
        agreement_version_id: body.agreement_version_id,
        accepted_at: body.accepted_at,
        ip: body.ip,
        user_agent: body.user_agent,
        metadata: body.metadata,
    };

    Ok(OrderAgreementView::from(
        order::accept_agreement(tx, ctx, order_id, acceptance).await?,
    ))
}

pub async fn order_agreements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderAgreementView>> {
    Ok(order::agreements(tx, ctx, order_id)
        .await?
        .into_iter()
        .map(OrderAgreementView::from)
        .collect())
}

/// The text this order's buyer read, whatever has been published since.
pub async fn accepted_text(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    kind: &str,
) -> Result<AgreementVersionView> {
    let kind = order::AgreementKind::parse(kind)?;

    Ok(AgreementVersionView::from(
        order::accepted_text(tx, ctx, order_id, kind).await?,
    ))
}

// ---------------------------------------------------------------------------
// Withdrawal
// ---------------------------------------------------------------------------

pub async fn withdrawal_windows(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<WithdrawalView>> {
    Ok(order::withdrawal_deadline(tx, ctx, order_id)
        .await?
        .into_iter()
        .map(WithdrawalView::from)
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawalNoticeView {
    pub return_id: ReturnId,
    pub order_id: OrderId,
    pub notified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub refund_due_by: Option<chrono::DateTime<chrono::Utc>>,
}

/// The buyer says they are withdrawing, which is the moment the refund starts
/// being owed.
pub async fn notify_withdrawal(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    return_id: ReturnId,
) -> Result<WithdrawalNoticeView> {
    let notified = order::notify_withdrawal(tx, ctx, return_id).await?;

    Ok(WithdrawalNoticeView {
        return_id: notified.id,
        order_id: notified.order_id,
        notified_at: notified.notified_at,
        refund_due_by: notified.refund_due_by,
    })
}

// ---------------------------------------------------------------------------
// Invoices
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordInvoice {
    pub number: String,
    pub external_id: Option<String>,
    pub provider: Option<String>,
    pub status: String,
    pub total: Decimal,
    pub currency_code: String,
    pub issued_at: Option<chrono::DateTime<chrono::Utc>>,
    pub document_url: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

impl RecordInvoice {
    fn document(self) -> Result<order::NewInvoice> {
        Ok(order::NewInvoice {
            number: self.number,
            external_id: self.external_id,
            provider: self.provider,
            status: order::InvoiceStatus::parse(&self.status)?,
            total: Money::new(self.total, Currency::parse(&self.currency_code)?),
            issued_at: self.issued_at,
            document_url: self.document_url,
            metadata: self.metadata,
        })
    }
}

pub async fn record_invoice(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    body: RecordInvoice,
) -> Result<InvoiceView> {
    Ok(InvoiceView::from(
        order::record_invoice(tx, ctx, order_id, body.document()?).await?,
    ))
}

/// The document that reverses one already issued. Money goes back with it,
/// which is why it asks to settle rather than to write.
pub async fn record_credit_note(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    replaces: OrderInvoiceId,
    body: RecordInvoice,
) -> Result<InvoiceView> {
    Ok(InvoiceView::from(
        order::record_credit_note(tx, ctx, order_id, replaces, body.document()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetInvoiceStatus {
    pub status: String,
}

pub async fn set_invoice_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderInvoiceId,
    body: SetInvoiceStatus,
) -> Result<InvoiceView> {
    let status = order::InvoiceStatus::parse(&body.status)?;

    Ok(InvoiceView::from(
        order::set_invoice_status(tx, ctx, id, status).await?,
    ))
}

pub async fn list_invoices(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<InvoiceView>> {
    Ok(order::invoices(tx, ctx, order_id)
        .await?
        .into_iter()
        .map(InvoiceView::from)
        .collect())
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/agreements",
        action: Action::Write,
        domain: "order",
        summary: "Publish a version of a document",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/agreements",
        action: Action::View,
        domain: "order",
        summary: "List published versions",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/agreements/{id}",
        action: Action::View,
        domain: "order",
        summary: "Fetch one published version",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/agreements",
        action: Action::View,
        domain: "order",
        summary: "What this order's buyer accepted",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/agreements/{kind}",
        action: Action::View,
        domain: "order",
        summary: "The text this order's buyer accepted",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/withdrawal",
        action: Action::View,
        domain: "order",
        summary: "When each line's window to change their mind closes",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/returns/{id}/withdrawal",
        action: Action::Write,
        domain: "order",
        summary: "Record that the buyer is withdrawing",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/invoices",
        action: Action::View,
        domain: "order",
        summary: "List the documents issued for an order",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/orders/{id}/invoices",
        action: Action::Write,
        domain: "order",
        summary: "Record that an invoice was issued",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/orders/{id}/invoices/{invoice_id}/credit-note",
        action: Action::Settle,
        domain: "order",
        summary: "Record the document that reverses an invoice",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Patch,
        path: "/admin/invoices/{id}",
        action: Action::Write,
        domain: "order",
        summary: "Write down what the authority answered",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/orders/{id}/agreements",
        action: Action::Write,
        domain: "order",
        summary: "Accept a document against my order",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/orders/{id}/agreements/{kind}",
        action: Action::View,
        domain: "order",
        summary: "Read the text I accepted",
    },
];
