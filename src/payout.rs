//! Who a seller-scope's orders owe money to, why, and whether the host has
//! said it was paid.
//!
//! `payout_line` is an appended ledger, the shape `order_transaction`
//! already has — never updated, signed, and read by summing rather than by
//! trusting a column somebody kept in step. [`record_earning`] writes what a
//! captured order splits into: the seller's share and the marketplace's
//! commission, always adding back up to the amount captured because both come
//! out of one [`crate::money::allocate`] call. [`record_reversal`] is its
//! mirror for a refund, and it unwinds both parts — the seller's share and
//! the commission the marketplace no longer keeps — in the caller's own
//! transaction, because a refund that rolled back the seller's money without
//! rolling back the marketplace's cut would still show a commission earned on
//! money that was given back.
//!
//! What this module does not do, on purpose: it never calls a provider, a
//! bank or kasapay. [`create_payout`] records that the host has already paid
//! a seller — through whatever moved the money — and brings the running
//! balance down by that amount. tezgah's part ends at the ledger entry.
//!
//! **Commission.** `commission_rule` follows `price_rule`/`promotion_rule`: a
//! scoped rule table resolved by matching an attribute rather than a fourth
//! kind of rule invented for this. The match here is narrower than either —
//! a product's category, or nothing — so there is one rule per category and,
//! separately, at most one default for everything a category rule does not
//! name, the way `campaign_budget` holds one row per campaign rather than
//! `price_rule`'s general attribute/operator engine, which a single-category
//! match has no use for. A line whose product matches no category rule and no
//! default earns no commission — a marketplace that wants one has to say so.
//!
//! **What is deliberately simplified.** Commission is resolved once per order
//! at capture, from that order's own lines and their categories — not
//! re-derived from `cart`/`pricing` internals `settlement` does not otherwise
//! depend on. A product in more than one category resolves to its lowest
//! category id, arbitrarily but deterministically; a marketplace that needs
//! a real precedence between two categories on the same product is not served
//! by this yet. Shipping and tax are never commissioned — they pass to the
//! seller in full — only each line's `quantity * unit_price` is a commission
//! base; that mirrors how a marketplace's own commission agreement usually
//! reads (a cut of the goods sold, not of the carrier's fee or the tax
//! authority's share) and keeps `record_earning` from having to reconstruct
//! `cart::compute`'s tax and shipping split for a single split it does not
//! otherwise need. A refund does not re-resolve `commission_rule` — rules may
//! have moved since the money was captured — it unwinds capture by capture,
//! oldest first, each fragment at that capture's own commission ratio rather
//! than the order's blended one. The two are the same number while an order
//! has one capture, and only diverge once it is captured in parts across a
//! rate change; `payout_line.capture_id` is what freezes the fact a
//! reversal reads instead of recomputing it.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{CategoryId, CommissionRuleId, OrderId, PaymentId, PayoutId, PayoutLineId};
use crate::money::{self, Currency, Money};
use crate::order;
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};
use crate::store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommissionKind {
    Fixed,
    Percentage,
}

impl CommissionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CommissionKind::Fixed => "fixed",
            CommissionKind::Percentage => "percentage",
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "fixed" => Ok(CommissionKind::Fixed),
            "percentage" => Ok(CommissionKind::Percentage),
            other => Err(Error::invalid(format!(
                "{other:?} is not a commission kind"
            ))),
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CommissionRule {
    pub id: CommissionRuleId,
    pub category_id: Option<CategoryId>,
    #[sqlx(rename = "kind")]
    pub kind: String,
    pub value: Decimal,
    pub currency_code: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct NewCommissionRule {
    /// `None` sets the default every uncategorised, or otherwise unmatched,
    /// line falls back to.
    pub category_id: Option<CategoryId>,
    pub kind: CommissionKind,
    pub value: Decimal,
    /// Required for [`CommissionKind::Fixed`]; ignored for a percentage.
    pub currency_code: Option<Currency>,
}

/// One category's rule, or the scope's default when `category_id` is `None`.
/// Idempotent on that key, the way `campaign_budget` upserts one row per
/// campaign — a category never carries two competing rates at once.
pub async fn set_commission_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewCommissionRule,
) -> Result<CommissionRule> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    if new.value.is_sign_negative() {
        return Err(Error::invalid("a commission rate cannot be negative"));
    }
    if new.kind == CommissionKind::Percentage && new.value > Decimal::from(100) {
        return Err(Error::invalid("a percentage commission cannot exceed 100"));
    }
    if new.kind == CommissionKind::Fixed && new.currency_code.is_none() {
        return Err(Error::invalid("a fixed commission needs a currency"));
    }

    let conflict_target = if new.category_id.is_some() {
        "(scope, category_id) where category_id is not null"
    } else {
        "(scope) where category_id is null"
    };

    let rule = sqlx::query_as::<_, CommissionRule>(&format!(
        "insert into commission_rule (id, scope, category_id, kind, value, currency_code)
         values ($1, $2, $3, $4, $5, $6)
         on conflict {conflict_target} do update
           set kind = excluded.kind, value = excluded.value, currency_code = excluded.currency_code
         returning id, category_id, kind, value, currency_code, created_at"
    ))
    .bind(CommissionRuleId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(new.category_id.map(CategoryId::as_uuid))
    .bind(new.kind.as_str())
    .bind(new.value)
    .bind(new.currency_code.map(|code| code.as_str().to_string()))
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "commission_rule",
            entity_id: rule.id.as_uuid(),
            summary: serde_json::json!({ "kind": rule.kind, "value": rule.value.to_string() }),
        },
    )
    .await?;

    Ok(rule)
}

pub async fn commission_rules(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<CommissionRule>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, CommissionRule>(
        "select id, category_id, kind, value, currency_code, created_at
         from commission_rule
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

pub async fn remove_commission_rule(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: CommissionRuleId,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Pricing)?;

    sqlx::query("delete from commission_rule where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "commission_rule",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PayoutLine {
    pub id: PayoutLineId,
    pub order_id: Option<OrderId>,
    pub payout_id: Option<PayoutId>,
    pub amount: Decimal,
    pub currency_code: String,
    pub reference: String,
    pub reference_id: Option<Uuid>,
    pub capture_id: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Payout {
    pub id: PayoutId,
    pub amount: Decimal,
    pub currency_code: String,
    pub reference: String,
    pub reference_id: Uuid,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

struct CommissionBasis {
    quantity: i32,
    unit_price: Decimal,
    kind: Option<String>,
    value: Option<Decimal>,
}

/// Splits `total` into a seller's share and the marketplace's commission,
/// resolved from this order's own lines. The two always sum back to `total`:
/// both come out of one [`money::allocate`] call, commission's weight first
/// so the rounding remainder — never more than one minor unit — lands on the
/// marketplace's share rather than the seller's. A marketplace absorbing that
/// is the deliberate choice: a seller's payout should read as exactly their
/// resolved rate applied, not as their rate plus whatever rounding happened
/// to do that day.
async fn split(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    total: Money,
) -> Result<(Money, Money)> {
    let order = order::get(tx, ctx, order_id).await?;
    let exponent = store::exponent(tx, ctx, total.currency).await?;

    let lines: Vec<CommissionBasis> =
        sqlx::query_as::<_, (i32, Decimal, Option<String>, Option<Decimal>)>(
            "select i.quantity, coalesce(i.unit_price, l.unit_price) as unit_price,
                coalesce(specific.kind, fallback.kind) as kind,
                coalesce(specific.value, fallback.value) as value
         from order_item i
         join order_line_item l on l.scope = i.scope and l.id = i.order_line_item_id
         left join lateral (
             select category_id from product_category_link pcl
             where pcl.scope = i.scope and pcl.product_id = l.product_id
             order by category_id limit 1
         ) cat on true
         left join commission_rule specific
             on specific.scope = i.scope and specific.category_id = cat.category_id
         left join commission_rule fallback
             on fallback.scope = i.scope and fallback.category_id is null
         where i.scope = $1 and i.order_id = $2 and i.version = $3",
        )
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(order.version)
        .fetch_all(&mut **tx)
        .await?
        .into_iter()
        .map(|(quantity, unit_price, kind, value)| CommissionBasis {
            quantity,
            unit_price,
            kind,
            value,
        })
        .collect();

    let mut commission_total = Decimal::ZERO;
    for line in &lines {
        let subtotal = Decimal::from(line.quantity) * line.unit_price;
        let Some(value) = line.value else { continue };
        let commission = match line.kind.as_deref() {
            Some("fixed") => value.min(subtotal),
            Some("percentage") => subtotal * value / Decimal::from(100),
            _ => continue,
        };
        commission_total += commission;
    }

    commission_total = commission_total.clamp(Decimal::ZERO, total.amount.max(Decimal::ZERO));
    let seller_total = total.amount - commission_total;

    let parts = money::allocate(total, &[commission_total, seller_total], exponent)?;
    let commission = parts[0];
    let seller = parts[1];
    Ok((seller, commission))
}

/// Writes what a captured amount earns: the seller's share and the
/// marketplace's commission, in the caller's own transaction. Idempotent on
/// `(order_id, reference, capture_id)`, so a redelivered capture webhook does
/// not double a seller's earnings.
pub async fn record_earning(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    captured: Money,
    capture_id: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Settle, Resource::Payout { id: None })?;

    let (seller, commission) = split(tx, ctx, order_id, captured).await?;

    write_line(
        tx,
        ctx,
        order_id,
        "seller_share",
        capture_id,
        seller,
        None,
        Some(capture_id),
    )
    .await?;
    write_line(
        tx,
        ctx,
        order_id,
        "commission",
        capture_id,
        commission,
        None,
        Some(capture_id),
    )
    .await?;

    Ok(())
}

/// Unwinds a refund's share of both what the seller earned and what the
/// marketplace kept — capture by capture, oldest first, each fragment taking
/// that capture's own commission ratio rather than the order's blended one.
/// With one capture per order this is the same number the order's overall
/// ratio would give; the two diverge only once an order is captured across a
/// commission rate change, which is exactly the case a blended rate got
/// wrong.
///
/// `capture_id` is the attribution, frozen per fragment; `refund_id` stays
/// the event every fragment shares, so a redelivered refund webhook dedupes
/// per capture rather than only against the first fragment it wrote.
pub async fn record_reversal(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    payment_id: PaymentId,
    refunded: Money,
    refund_id: Uuid,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Settle, Resource::Payout { id: None })?;

    let exponent = store::exponent(tx, ctx, refunded.currency).await?;

    let captures: Vec<Uuid> = sqlx::query_scalar(
        "select id from capture
         where scope = $1 and payment_id = $2
         order by created_at, id",
    )
    .bind(ctx.scope.0)
    .bind(payment_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    let mut remaining = refunded.amount;
    for capture_id in captures {
        if remaining.is_zero() {
            break;
        }

        let totals: (Option<Decimal>, Option<Decimal>, Option<Decimal>) = sqlx::query_as(
            "select
                 sum(amount) filter (where reference = 'commission'),
                 sum(amount) filter (where reference in ('seller_share', 'commission')),
                 sum(-amount) filter (where reference in ('refund_seller_share', 'refund_commission'))
             from payout_line
             where scope = $1 and order_id = $2 and capture_id = $3",
        )
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(capture_id)
        .fetch_one(&mut **tx)
        .await?;

        let commission_earned = totals.0.unwrap_or(Decimal::ZERO);
        let captured_earned = totals.1.unwrap_or(Decimal::ZERO);
        let already_reversed = totals.2.unwrap_or(Decimal::ZERO);

        let capture_remaining = (captured_earned - already_reversed).max(Decimal::ZERO);
        if capture_remaining.is_zero() {
            continue;
        }

        let take = remaining.min(capture_remaining);
        let rate = if captured_earned.is_zero() {
            Decimal::ZERO
        } else {
            commission_earned / captured_earned
        };

        let commission_part = (take * rate).clamp(Decimal::ZERO, take.max(Decimal::ZERO));
        let seller_part = take - commission_part;
        let parts = money::allocate(
            Money::new(take, refunded.currency),
            &[commission_part, seller_part],
            exponent,
        )?;
        let commission = parts[0];
        let seller = parts[1];

        write_line(
            tx,
            ctx,
            order_id,
            "refund_seller_share",
            refund_id,
            Money::new(-seller.amount, seller.currency),
            None,
            Some(capture_id),
        )
        .await?;
        write_line(
            tx,
            ctx,
            order_id,
            "refund_commission",
            refund_id,
            Money::new(-commission.amount, commission.currency),
            None,
            Some(capture_id),
        )
        .await?;

        remaining -= take;
    }

    if !remaining.is_zero() {
        return Err(Error::bug(
            "a refund did not fully attribute to a capture on this payment",
        ));
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn write_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    reference: &str,
    reference_id: Uuid,
    amount: Money,
    payout_id: Option<PayoutId>,
    capture_id: Option<Uuid>,
) -> Result<()> {
    if amount.amount.is_zero() {
        return Ok(());
    }

    sqlx::query(
        "insert into payout_line
             (id, scope, order_id, payout_id, amount, currency_code, reference, reference_id,
              capture_id)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         on conflict (scope, order_id, reference, reference_id, capture_id)
             where reference_id is not null and order_id is not null
         do nothing",
    )
    .bind(PayoutLineId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(payout_id.map(PayoutId::as_uuid))
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(reference)
    .bind(reference_id)
    .bind(capture_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// What this scope is owed right now: every accrual and payout summed. Never
/// filtered to the positive side — a seller paid ahead of a refund that later
/// lands owes the marketplace, and the next payout has to see that.
pub async fn balance(tx: &mut Tx<'_>, ctx: &Ctx<'_>, currency: Currency) -> Result<Money> {
    let _: Permit = ctx.permit(Action::View, Resource::Payout { id: None })?;

    let total: Option<Decimal> = sqlx::query_scalar(
        "select sum(amount) from payout_line
         where scope = $1 and currency_code = $2
           and reference in ('seller_share', 'refund_seller_share', 'paid_out')",
    )
    .bind(ctx.scope.0)
    .bind(currency.as_str())
    .fetch_one(&mut **tx)
    .await?;

    Ok(Money::new(total.unwrap_or(Decimal::ZERO), currency))
}

/// Records that the host has already paid this scope's outstanding balance —
/// through kasapay, a bank transfer, anything that actually moved money —
/// and writes the ledger row that brings it back to zero. tezgah never calls
/// a provider here; `reference`/`reference_id` are the host's own record of
/// the transfer, the same way `order_transaction.reference` names a payment
/// tezgah did not initiate either.
pub async fn create_payout(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    currency: Currency,
    reference: &str,
    reference_id: Uuid,
    metadata: Option<serde_json::Value>,
) -> Result<Payout> {
    let _: Permit = ctx.permit(Action::Settle, Resource::Payout { id: None })?;

    let owed = balance(tx, ctx, currency).await?;
    if owed.amount <= Decimal::ZERO {
        return Err(Error::invalid(
            "nothing owed to this scope in that currency",
        ));
    }

    let payout = sqlx::query_as::<_, Payout>(
        "insert into payout (id, scope, amount, currency_code, reference, reference_id, metadata)
         values ($1, $2, $3, $4, $5, $6, $7)
         returning id, amount, currency_code, reference, reference_id, created_at",
    )
    .bind(PayoutId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(owed.amount)
    .bind(owed.currency.as_str())
    .bind(reference)
    .bind(reference_id)
    .bind(metadata)
    .fetch_one(&mut **tx)
    .await?;

    sqlx::query(
        "insert into payout_line
             (id, scope, order_id, payout_id, amount, currency_code, reference, reference_id)
         values ($1, $2, null, $3, $4, $5, 'paid_out', $6)",
    )
    .bind(PayoutLineId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(payout.id.as_uuid())
    .bind(-owed.amount)
    .bind(owed.currency.as_str())
    .bind(reference_id)
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "payout",
            entity_id: payout.id.as_uuid(),
            summary: serde_json::json!({ "amount": payout.amount.to_string() }),
        },
    )
    .await?;

    Ok(payout)
}

pub async fn payouts(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<Payout>> {
    let _: Permit = ctx.permit(Action::View, Resource::Payout { id: None })?;

    let rows = sqlx::query_as::<_, Payout>(
        "select id, amount, currency_code, reference, reference_id, created_at
         from payout
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

pub async fn payout_lines(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    paging: Paging,
) -> Result<Page<PayoutLine>> {
    let _: Permit = ctx.permit(Action::View, Resource::Payout { id: None })?;

    let rows = sqlx::query_as::<_, PayoutLine>(
        "select id, order_id, payout_id, amount, currency_code, reference, reference_id,
                capture_id, created_at
         from payout_line
         where scope = $1 and order_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
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
