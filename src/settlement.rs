//! What happens because money arrived.
//!
//! [`capture`] takes the money — by way of
//! [`payment::capture_only`](crate::payment::capture_only) — and then, in the
//! same transaction, does everything a captured payment obligates the shop
//! to: print a gift card that was bought rather than earned, grant a digital
//! entitlement, start a subscription's first period. [`refund`] is the
//! mirror, with one deliberate asymmetry: a refund revokes what capturing
//! granted, but it does not un-print a card. A captured-then-refunded card is
//! left alone.
//!
//! This is the top of the graph on purpose. `payment::capture_only` and
//! `payment::refund_only` do not know about gift cards, entitlements or
//! subscriptions — teaching them would put `payment` in a cycle with
//! `credit` and `digital`. Nothing may depend on `settlement`; a route or a
//! host's webhook handler calls it and nothing beneath it.

use serde_json::Value;
use uuid::Uuid;

use crate::credit;
use crate::digital;
use crate::error::Result;
use crate::id::{
    OrderId, PaymentCollectionId, PaymentId, RefundReasonId, SubscriptionId, SubscriptionOrderId,
};
use crate::money::Money;
use crate::order;
use crate::payment;
use crate::ports::{Action, Ctx, Permit, Resource, Tx};
use crate::subscription;

/// The order a payment's collection is attached to, or nil when there is
/// none yet — a collection may be opened before checkout finishes it. The
/// lookup answers nothing to the caller; the permission is taken with what
/// it found, immediately after.
async fn order_of(tx: &mut Tx<'_>, ctx: &Ctx<'_>, collection: PaymentCollectionId) -> Result<Uuid> {
    let order_id: Option<Uuid> = sqlx::query_scalar(
        r#"select id from "order" where scope = $1 and payment_collection_id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(collection.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(order_id.unwrap_or_else(Uuid::nil))
}

/// The entry point for money arriving.
///
/// [`Action::Settle`]. The permission is asked here, on the payment, before
/// anything else in this call writes a row — the same question
/// `payment::capture_only` asks again beneath it, because that function is
/// still callable (from tests, and from `payment` itself) without going
/// through settlement.
///
/// The order judged on is the one the payment's collection is already
/// attached to: by the time anything reaches capture, checkout has placed
/// it, so an authorizer that wants to decide by order gets a real one rather
/// than a nil it cannot judge.
pub async fn capture(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    amount: Money,
    metadata: Option<Value>,
) -> Result<payment::Capture> {
    let collection = payment::payment(tx, ctx, id).await?.payment_collection_id;
    let order = order_of(tx, ctx, collection).await?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Payment {
            id: id.as_uuid(),
            order,
            customer: None,
        },
    )?;

    let capture = payment::capture_only(tx, ctx, id, amount, metadata).await?;

    let written = order::record_capture(tx, ctx, collection, capture.id, amount).await?;

    if let Some(transaction) = written {
        credit::issue_purchased(tx, ctx, transaction.order_id).await?;
        digital::grant(tx, ctx, transaction.order_id).await?;
        start_first_period(tx, ctx, transaction.order_id).await?;
    }

    Ok(capture)
}

/// Gives money back and revokes what capturing granted. Leaves a purchased
/// gift card alone: capture has no compensation in this crate, and a refund
/// pretending otherwise would print money the shop never took back.
pub async fn refund(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    amount: Money,
    reason: Option<RefundReasonId>,
    note: Option<String>,
) -> Result<payment::Refund> {
    let collection = payment::payment(tx, ctx, id).await?.payment_collection_id;
    let order = order_of(tx, ctx, collection).await?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Payment {
            id: id.as_uuid(),
            order,
            customer: None,
        },
    )?;

    let refund = payment::refund_only(tx, ctx, id, amount, reason, note).await?;

    let written = order::record_refund(tx, ctx, collection, refund.id, amount).await?;

    if let Some(transaction) = written {
        digital::revoke(tx, ctx, transaction.order_id, Some("refunded")).await?;
    }

    Ok(refund)
}

/// A subscription's first period runs from the moment
/// [`subscription::create`](crate::subscription::create) opens the contract,
/// unlike every later one, which a renewal bills before it starts. Nothing
/// bills this one, so the first capture on the order that sold it does:
/// `cycle` 0 is that period, and `subscription_order`'s existing unique key
/// on `(scope, subscription_id, cycle)` is what makes a redelivered webhook a
/// no-op rather than a second period.
///
/// An order is under a contract when its `subscription_id` column names
/// one — set on the orders `subscription`'s own `create_order` step places,
/// and checked by `tezgah_fk` the way every other reference here is, so the
/// id can never name a contract outside this scope.
async fn start_first_period(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<()> {
    let subscription_id: Option<Uuid> =
        sqlx::query_scalar(r#"select subscription_id from "order" where scope = $1 and id = $2"#)
            .bind(ctx.scope.0)
            .bind(order_id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?
            .flatten();

    let Some(subscription_id) = subscription_id else {
        return Ok(());
    };

    let contract = subscription::get(tx, ctx, SubscriptionId::from_uuid(subscription_id)).await?;

    sqlx::query(
        "insert into subscription_order
             (id, scope, subscription_id, order_id, cycle, period_start, period_end, kind)
         values ($1, $2, $3, $4, 0, $5, $6, 'initial')
         on conflict (scope, subscription_id, cycle) do nothing",
    )
    .bind(SubscriptionOrderId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(subscription_id)
    .bind(order_id.as_uuid())
    .bind(contract.current_period_start)
    .bind(contract.current_period_end)
    .execute(&mut **tx)
    .await?;

    Ok(())
}
