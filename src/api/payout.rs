//! The admin surface for commission rules and a seller-scope's own payout
//! ledger.
//!
//! There is no store-facing route here: a shopper never sees what a seller
//! was paid. What lives here is a seller's own back office reading what it
//! is owed and why, and the host recording — never initiating — that money
//! actually moved.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::id::{CategoryId, CommissionRuleId, OrderId, PayoutId, PayoutLineId};
use crate::money::Currency;
use crate::page::{Cursor, Page, Paging};
use crate::payout::{self, CommissionKind, NewCommissionRule};
use crate::ports::{Action, Ctx, Tx};

use super::{Method, Route, Surface};

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommissionRuleView {
    pub id: CommissionRuleId,
    pub category_id: Option<CategoryId>,
    pub kind: String,
    pub value: rust_decimal::Decimal,
    pub currency_code: Option<String>,
}

impl From<payout::CommissionRule> for CommissionRuleView {
    fn from(row: payout::CommissionRule) -> Self {
        CommissionRuleView {
            id: row.id,
            category_id: row.category_id,
            kind: row.kind,
            value: row.value,
            currency_code: row.currency_code,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SetCommissionRule {
    pub category_id: Option<CategoryId>,
    pub kind: String,
    pub value: rust_decimal::Decimal,
    pub currency_code: Option<String>,
}

pub async fn set_commission_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: SetCommissionRule,
) -> Result<CommissionRuleView> {
    let new = NewCommissionRule {
        category_id: body.category_id,
        kind: CommissionKind::parse(&body.kind)?,
        value: body.value,
        currency_code: body
            .currency_code
            .map(|code| Currency::parse(&code))
            .transpose()?,
    };
    Ok(payout::set_commission_rule(tx, ctx, new).await?.into())
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

pub async fn commission_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListQuery,
) -> Result<Page<CommissionRuleView>> {
    let page =
        payout::commission_rules(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page
            .items
            .into_iter()
            .map(CommissionRuleView::from)
            .collect(),
        next: page.next,
    })
}

pub async fn remove_commission_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CommissionRuleId,
) -> Result<()> {
    payout::remove_commission_rule(tx, ctx, id).await
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutLineView {
    pub id: PayoutLineId,
    pub order_id: Option<OrderId>,
    pub payout_id: Option<PayoutId>,
    pub amount: rust_decimal::Decimal,
    pub currency_code: String,
    pub reference: String,
    pub reference_id: Option<Uuid>,
}

impl From<payout::PayoutLine> for PayoutLineView {
    fn from(row: payout::PayoutLine) -> Self {
        PayoutLineView {
            id: row.id,
            order_id: row.order_id,
            payout_id: row.payout_id,
            amount: row.amount,
            currency_code: row.currency_code,
            reference: row.reference,
            reference_id: row.reference_id,
        }
    }
}

pub async fn order_payout_lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    query: ListQuery,
) -> Result<Page<PayoutLineView>> {
    let page = payout::payout_lines(
        tx,
        ctx,
        order_id,
        paging(query.after.as_deref(), query.limit)?,
    )
    .await?;
    Ok(Page {
        items: page.items.into_iter().map(PayoutLineView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutView {
    pub id: PayoutId,
    pub amount: rust_decimal::Decimal,
    pub currency_code: String,
    pub reference: String,
    pub reference_id: Uuid,
}

impl From<payout::Payout> for PayoutView {
    fn from(row: payout::Payout) -> Self {
        PayoutView {
            id: row.id,
            amount: row.amount,
            currency_code: row.currency_code,
            reference: row.reference,
            reference_id: row.reference_id,
        }
    }
}

pub async fn payouts(tx: &mut Tx<'_>, ctx: &Ctx<'_>, query: ListQuery) -> Result<Page<PayoutView>> {
    let page = payout::payouts(tx, ctx, paging(query.after.as_deref(), query.limit)?).await?;
    Ok(Page {
        items: page.items.into_iter().map(PayoutView::from).collect(),
        next: page.next,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatePayout {
    pub currency_code: String,
    pub reference: String,
    pub reference_id: Uuid,
    pub metadata: Option<serde_json::Value>,
}

/// Records that the host has already paid this scope's outstanding balance —
/// tezgah never moves the money itself.
pub async fn create_payout(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: CreatePayout,
) -> Result<PayoutView> {
    let currency = Currency::parse(&body.currency_code)?;
    Ok(payout::create_payout(
        tx,
        ctx,
        currency,
        &body.reference,
        body.reference_id,
        body.metadata,
    )
    .await?
    .into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceView {
    pub amount: rust_decimal::Decimal,
    pub currency_code: String,
}

/// What this scope is owed right now, in one currency — negative when a
/// refund outran what was already paid out.
pub async fn balance(tx: &mut Tx<'_>, ctx: &Ctx<'_>, currency_code: String) -> Result<BalanceView> {
    let currency = Currency::parse(&currency_code)?;
    let owed = payout::balance(tx, ctx, currency).await?;
    Ok(BalanceView {
        amount: owed.amount,
        currency_code: owed.currency.as_str().to_string(),
    })
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/commission-rules",
        action: Action::Write,
        domain: "payout",
        summary: "Set the commission rate for a category, or the scope's default",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/commission-rules",
        action: Action::View,
        domain: "payout",
        summary: "This scope's commission rules",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Delete,
        path: "/admin/commission-rules/{id}",
        action: Action::Delete,
        domain: "payout",
        summary: "Remove a commission rule",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/orders/{id}/payout-lines",
        action: Action::View,
        domain: "payout",
        summary: "What one order earned this scope and what it cost in commission",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/payouts",
        action: Action::View,
        domain: "payout",
        summary: "This scope's own history of money the host has said left the shop",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/payouts",
        action: Action::Settle,
        domain: "payout",
        summary: "Record that the host has already paid this scope's balance",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/payout-balance/{currency_code}",
        action: Action::View,
        domain: "payout",
        summary: "What this scope is owed right now, in one currency",
    },
];
