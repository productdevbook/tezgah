//! Taking money, and being able to say afterwards exactly what was taken.
//!
//! The shape is [`store`](crate::store)'s: transaction first, context second,
//! a [`Permit`] before any row is touched, `scope` named in every predicate,
//! and audit rows and events written in the caller's transaction.
//!
//! Two things are different here, and both are because money is on the other
//! side of a network.
//!
//! **Authorising and capturing are separate calls with separate permissions.**
//! An authorisation is a promise the provider holds; a capture is the money
//! leaving the cardholder. They are answered as [`Action::Write`] and
//! [`Action::Settle`] respectively, so a role that can edit an order is not
//! thereby a role that can charge one.
//!
//! **A capture has no compensation.** Nothing in this module will undo one: a
//! captured amount is given back by a [`refund`], which is another row and
//! another provider call, and pretending otherwise would leave the ledger
//! disagreeing with the bank.
//!
//! # When the provider disagrees about the amount
//!
//! A provider that reports a different amount from the one it was asked for has
//! still moved that money. Refusing the write would leave a paid customer with
//! a pending order, so the disagreement is recorded — the collection goes to
//! `mismatch` and `payment.amount_mismatch` is emitted — and somebody is told.
//! See [`flag_mismatch`].
//!
//! What counts as a disagreement is not "the amounts differ". A card split
//! into instalments is authorised for the basket plus a bank-set difference —
//! "vade farkı" — which the shopper agreed to, so the guard compares what was
//! held against what was agreed: the order total plus the surcharge in
//! [`Installment`]. An unexplained difference is still a mismatch.
//!
//! Nothing here validates the plan itself. Which counts a card may be split
//! into is a regulator's table, revised several times a year and applied by
//! sector; a library that carried it would refuse sales a bank allowed. What
//! is stored is what the provider said it accepted.
//!
//! # Webhooks
//!
//! [`record_webhook`] writes the event before anything acts on it, and the
//! unique index on `(scope, provider, event_id)` is what makes the provider's
//! second delivery a no-op rather than a second capture. The flow is:
//!
//! 1. [`PaymentProvider::parse_webhook`] verifies the signature and says what
//!    the event means.
//! 2. [`record_webhook`] returns [`WebhookOutcome::AlreadySeen`] — stop, answer
//!    the provider 200 — or [`WebhookOutcome::Fresh`]. That transaction commits
//!    on its own: an event row that rolls back with the work it was recording
//!    is an event nobody will ever deliver again.
//! 3. The caller acts on it, in a transaction of its own.
//! 4. [`mark_processed`] on success, [`mark_failed`] on failure.
//!
//! A failed event keeps `processed_at` null, which is what [`unprocessed`]
//! lists. That is the retry path, and it is the only one: the provider's own
//! redelivery of the same `event_id` is `AlreadySeen` by then. It is also what
//! makes an out-of-order delivery survivable — a capture that arrives before
//! its authorisation fails, stays unprocessed, and applies when replayed.

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    AccountHolderId, CaptureId, CartId, CustomerId, PaymentCollectionId, PaymentId,
    PaymentProviderId, PaymentSessionId, PaymentWebhookEventId, RefundId, RefundReasonId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Actor, AuditEntry, Ctx, Event, Permit, Resource, Tx};

/// Providers are configuration, not a customer's data: a shop with more than
/// this has a different problem from a missing page.
const MAX_PROVIDERS: i64 = 200;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Provider {
    pub id: PaymentProviderId,
    pub code: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PaymentCollection {
    pub id: PaymentCollectionId,
    pub cart_id: Option<CartId>,
    pub currency_code: String,
    pub amount: Decimal,
    pub authorized_amount: Option<Decimal>,
    pub captured_amount: Option<Decimal>,
    pub refunded_amount: Option<Decimal>,
    pub status: String,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// What the shopper chose to split the card into, as the provider accepted
    /// it. No ceiling is held here: the counts a card may be split into are a
    /// regulator's, they change several times a year, and a library carrying
    /// last year's table would refuse a sale a bank allowed.
    pub installment_count: Option<i32>,
    /// The instalment difference the shopper pays on top — "vade farkı".
    pub surcharge_amount: Decimal,
    /// The instalment difference the shop pays out of its settlement instead.
    pub merchant_surcharge_amount: Decimal,
    /// `amount` plus the customer-borne surcharge: what the card is charged.
    pub charged_amount: Option<Decimal>,
}

impl PaymentCollection {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn status(&self) -> CollectionStatus {
        CollectionStatus::parse(&self.status)
    }

    pub fn total(&self) -> Result<Money> {
        Ok(Money::new(self.amount, self.currency()?))
    }

    pub fn captured(&self) -> Decimal {
        self.captured_amount.unwrap_or(Decimal::ZERO)
    }

    pub fn refunded(&self) -> Decimal {
        self.refunded_amount.unwrap_or(Decimal::ZERO)
    }

    pub fn authorized(&self) -> Decimal {
        self.authorized_amount.unwrap_or(Decimal::ZERO)
    }

    /// What the card is charged: the basket plus whatever instalment
    /// difference the shopper agreed to carry.
    pub fn charged(&self) -> Decimal {
        self.charged_amount
            .unwrap_or(self.amount + self.surcharge_amount)
    }

    pub fn charged_total(&self) -> Result<Money> {
        Ok(Money::new(self.charged(), self.currency()?))
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PaymentSession {
    pub id: PaymentSessionId,
    pub payment_collection_id: PaymentCollectionId,
    pub payment_provider_id: PaymentProviderId,
    pub currency_code: String,
    pub amount: Decimal,
    pub status: String,
    pub data: Value,
    pub context: Option<Value>,
    pub authorized_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// What the shopper asked the card to be split into, before the bank
    /// answered.
    pub installment_count: Option<i32>,
}

impl PaymentSession {
    pub fn status(&self) -> SessionStatus {
        SessionStatus::parse(&self.status)
    }

    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn money(&self) -> Result<Money> {
        Ok(Money::new(self.amount, self.currency()?))
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Payment {
    pub id: PaymentId,
    pub payment_collection_id: PaymentCollectionId,
    pub payment_session_id: Option<PaymentSessionId>,
    pub payment_provider_id: PaymentProviderId,
    pub currency_code: String,
    /// What the card was charged, instalment difference included. The goods
    /// are worth `amount - surcharge_amount` when the shopper carried it, and
    /// both numbers are needed: the invoice is for the goods and the bank
    /// statement is for the charge.
    pub amount: Decimal,
    pub data: Option<Value>,
    pub captured_at: Option<chrono::DateTime<chrono::Utc>>,
    pub canceled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub installment_count: Option<i32>,
    pub surcharge_amount: Decimal,
    pub surcharge_bearer: Option<String>,
    /// The provider's own name for the campaign or the card range it applied.
    /// Opaque: whose card qualifies for which plan is the bank's business.
    pub installment_campaign: Option<String>,
}

impl Payment {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn money(&self) -> Result<Money> {
        Ok(Money::new(self.amount, self.currency()?))
    }

    pub fn bearer(&self) -> Option<SurchargeBearer> {
        self.surcharge_bearer.as_deref().map(SurchargeBearer::parse)
    }

    /// What the goods came to: the charge less any difference the shopper
    /// carried for splitting it. A merchant-funded plan does not move this —
    /// the shop gave up part of the settlement, and the buyer paid the basket.
    pub fn goods_amount(&self) -> Decimal {
        match self.bearer() {
            Some(SurchargeBearer::Customer) => self.amount - self.surcharge_amount,
            _ => self.amount,
        }
    }

    /// What to give back on the card when `goods` worth of the sale is being
    /// undone.
    ///
    /// A refund is settled against what the card was charged, not against the
    /// price tag: the bank credits the rail it debited, and refunding the
    /// goods alone would leave the shopper paying a difference for instalments
    /// they no longer have. So the surcharge is returned in the same
    /// proportion as the goods.
    pub fn charge_for_goods(&self, goods: Money) -> Result<Money> {
        let currency = self.currency()?;
        if goods.currency != currency {
            return Err(Error::bug("a refund met another currency"));
        }
        let whole = self.goods_amount();
        if whole <= Decimal::ZERO || goods.amount >= whole {
            return Ok(Money::new(self.amount, currency));
        }
        let share = (self.amount * goods.amount / whole).round_dp(6);
        Ok(Money::new(share, currency))
    }
}

/// Who pays for splitting a card into instalments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurchargeBearer {
    /// The shopper, on top of the basket: the card is charged more than the
    /// order came to, and that is not a mismatch.
    Customer,
    /// The shop, out of the settlement: the card is charged the basket and
    /// less money arrives.
    Merchant,
}

impl SurchargeBearer {
    pub fn as_str(self) -> &'static str {
        match self {
            SurchargeBearer::Customer => "customer",
            SurchargeBearer::Merchant => "merchant",
        }
    }

    pub fn parse(text: &str) -> SurchargeBearer {
        match text {
            "customer" => SurchargeBearer::Customer,
            _ => SurchargeBearer::Merchant,
        }
    }
}

/// What a provider says it did about splitting the card.
///
/// No count, ceiling or sector rule is validated here. Which counts a card may
/// be split into is set by a regulator — in Turkey by sector and card range,
/// revised several times a year — and belongs to whoever talks to the bank.
/// tezgah records what was accepted.
#[derive(Debug, Clone)]
pub struct Installment {
    pub count: i32,
    /// The difference, in the collection's currency. Zero for an interest-free
    /// plan nobody funded.
    pub surcharge: Money,
    pub bearer: SurchargeBearer,
    pub campaign: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Capture {
    pub id: CaptureId,
    pub payment_id: PaymentId,
    pub amount: Decimal,
    pub currency_code: String,
    pub created_by: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Refund {
    pub id: RefundId,
    pub payment_id: PaymentId,
    pub refund_reason_id: Option<RefundReasonId>,
    pub amount: Decimal,
    pub currency_code: String,
    pub note: Option<String>,
    pub created_by: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// The customer as one provider knows them, which is what makes a saved card
/// reusable.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AccountHolder {
    pub id: AccountHolderId,
    pub payment_provider_id: PaymentProviderId,
    pub customer_id: Option<CustomerId>,
    pub external_id: String,
    pub email: Option<String>,
    pub data: Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// What a payment is worth right now: authorised, minus what has been taken,
/// minus what has been given back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Balance {
    pub authorized: Decimal,
    pub captured: Decimal,
    pub refunded: Decimal,
}

impl Balance {
    /// What may still be captured against the authorisation.
    pub fn capturable(&self) -> Decimal {
        self.authorized - self.captured
    }

    /// What may still be refunded. Never more than was actually taken.
    pub fn refundable(&self) -> Decimal {
        self.captured - self.refunded
    }
}

// ---------------------------------------------------------------------------
// States
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    /// The shopper has somewhere else to be — a 3-D Secure page, a bank app.
    /// Not a failure: the same session is authorised again when they come back.
    RequiresMore,
    Authorized,
    Captured,
    Canceled,
    Error,
}

impl SessionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            SessionStatus::Pending => "pending",
            SessionStatus::RequiresMore => "requires_more",
            SessionStatus::Authorized => "authorized",
            SessionStatus::Captured => "captured",
            SessionStatus::Canceled => "canceled",
            SessionStatus::Error => "error",
        }
    }

    pub fn parse(text: &str) -> SessionStatus {
        match text {
            "requires_more" => SessionStatus::RequiresMore,
            "authorized" => SessionStatus::Authorized,
            "captured" => SessionStatus::Captured,
            "canceled" => SessionStatus::Canceled,
            "error" => SessionStatus::Error,
            _ => SessionStatus::Pending,
        }
    }

    /// Whether the shopper can still be sent back to finish this one.
    pub fn is_open(self) -> bool {
        matches!(self, SessionStatus::Pending | SessionStatus::RequiresMore)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CollectionStatus {
    NotPaid,
    Awaiting,
    PartiallyAuthorized,
    Authorized,
    PartiallyCaptured,
    Captured,
    PartiallyRefunded,
    Refunded,
    Canceled,
    Failed,
    /// The provider and the shop disagree about how much was moved. Needs a
    /// person; nothing here clears it.
    Mismatch,
}

impl CollectionStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            CollectionStatus::NotPaid => "not_paid",
            CollectionStatus::Awaiting => "awaiting",
            CollectionStatus::PartiallyAuthorized => "partially_authorized",
            CollectionStatus::Authorized => "authorized",
            CollectionStatus::PartiallyCaptured => "partially_captured",
            CollectionStatus::Captured => "captured",
            CollectionStatus::PartiallyRefunded => "partially_refunded",
            CollectionStatus::Refunded => "refunded",
            CollectionStatus::Canceled => "canceled",
            CollectionStatus::Failed => "failed",
            CollectionStatus::Mismatch => "mismatch",
        }
    }

    pub fn parse(text: &str) -> CollectionStatus {
        match text {
            "awaiting" => CollectionStatus::Awaiting,
            "partially_authorized" => CollectionStatus::PartiallyAuthorized,
            "authorized" => CollectionStatus::Authorized,
            "partially_captured" => CollectionStatus::PartiallyCaptured,
            "captured" => CollectionStatus::Captured,
            "partially_refunded" => CollectionStatus::PartiallyRefunded,
            "refunded" => CollectionStatus::Refunded,
            "canceled" => CollectionStatus::Canceled,
            "failed" => CollectionStatus::Failed,
            "mismatch" => CollectionStatus::Mismatch,
            _ => CollectionStatus::NotPaid,
        }
    }
}

// ---------------------------------------------------------------------------
// The provider
// ---------------------------------------------------------------------------

/// What tezgah asks of a Stripe, an iyzico, a cash-on-delivery counter.
///
/// No method takes a [`Tx`]: these calls go out over a network and a database
/// lock must not be held while one is waited on. The caller — a workflow step —
/// makes the call, then writes the answer into its own transaction with the
/// functions in this module.
#[async_trait]
pub trait PaymentProvider: Send + Sync {
    /// Stable, and what the `payment_provider` row is found by. Renaming it is
    /// a migration, not an edit.
    fn code(&self) -> &'static str;

    async fn create_session(&self, req: SessionRequest) -> Result<SessionResponse>;

    async fn authorize(&self, req: AuthorizeRequest) -> Result<Authorization>;

    /// Separate from [`PaymentProvider::authorize`] on purpose, and partial:
    /// three shipments off one authorisation are three captures.
    async fn capture(&self, req: CaptureRequest) -> Result<CaptureResult>;

    async fn refund(&self, req: RefundRequest) -> Result<RefundResult>;

    async fn cancel(&self, req: CancelRequest) -> Result<()>;

    /// Verifies a signature and returns what the event means, or refuses.
    ///
    /// Refusing is the security boundary: an event whose signature does not
    /// check out must never reach [`record_webhook`].
    fn parse_webhook(&self, headers: &[(String, String)], body: &[u8]) -> Result<WebhookEvent>;
}

#[derive(Debug, Clone)]
pub struct SessionRequest {
    pub session_id: PaymentSessionId,
    pub collection_id: PaymentCollectionId,
    pub amount: Money,
    /// Whatever the host knows about the shopper: an email, a return url, an
    /// address. Opaque here.
    pub context: Value,
    /// The provider's own id for this customer, when there is one.
    pub account_holder: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionResponse {
    /// The provider's state — an intent id, a redirect url. Stored as given.
    pub data: Value,
    pub status: SessionStatus,
}

#[derive(Debug, Clone)]
pub struct AuthorizeRequest {
    pub session_id: PaymentSessionId,
    pub amount: Money,
    pub data: Value,
    /// What came back from the shopper's browser after a redirect.
    pub context: Value,
    /// What the shopper asked the card to be split into, when they did.
    pub installment_count: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationStatus {
    Authorized,
    /// 3-D Secure, a bank app, a second factor. The session stays open and
    /// nothing is canceled: the shopper comes back and it is tried again.
    RequiresMore,
    Error,
}

#[derive(Debug, Clone)]
pub struct Authorization {
    pub status: AuthorizationStatus,
    /// What the provider says it is holding. Compared with what it was asked
    /// for, and a disagreement is recorded rather than refused.
    pub amount: Option<Money>,
    pub data: Value,
    /// Where to send the shopper next, when `status` is `RequiresMore`.
    pub redirect: Option<String>,
    pub message: Option<String>,
    /// The plan the bank accepted, when the card was split. Its surcharge is
    /// what makes an amount larger than the order total an agreed price rather
    /// than a mismatch.
    pub installment: Option<Installment>,
}

#[derive(Debug, Clone)]
pub struct CaptureRequest {
    pub payment_id: PaymentId,
    pub amount: Money,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct CaptureResult {
    pub amount: Money,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct RefundRequest {
    pub payment_id: PaymentId,
    pub amount: Money,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct RefundResult {
    pub amount: Money,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub struct CancelRequest {
    pub session_id: Option<PaymentSessionId>,
    pub payment_id: Option<PaymentId>,
    pub data: Value,
}

/// What a provider's webhook turned out to mean, once its signature checked out.
#[derive(Debug, Clone)]
pub struct WebhookEvent {
    /// The provider's own id for this delivery. The same one arrives again.
    pub event_id: String,
    pub kind: WebhookKind,
    /// The provider's name for it, kept verbatim for the audit trail.
    pub event_type: String,
    pub session_id: Option<PaymentSessionId>,
    pub amount: Option<Money>,
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookKind {
    Authorized,
    Captured,
    Refunded,
    Canceled,
    Failed,
    /// Something tezgah does not model. Recorded, acknowledged, ignored.
    Other,
}

/// Whether this delivery is the first one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookOutcome {
    /// Written now, and nothing has acted on it yet.
    Fresh { id: PaymentWebhookEventId },
    /// Seen before. The caller does nothing at all — that is the whole point.
    AlreadySeen,
}

impl WebhookOutcome {
    pub fn is_fresh(&self) -> bool {
        matches!(self, WebhookOutcome::Fresh { .. })
    }
}

/// One event still waiting to be acted on.
#[derive(Debug, Clone, FromRow)]
pub struct PendingWebhook {
    pub id: PaymentWebhookEventId,
    pub payment_provider_id: PaymentProviderId,
    pub event_id: String,
    pub event_type: String,
    pub payload: Value,
    pub attempts: i32,
    pub received_at: chrono::DateTime<chrono::Utc>,
}

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct NewCollection {
    pub amount: Money,
    /// The cart this is being opened for, when there is one. It is what the
    /// collection's owner is later read from.
    pub cart_id: Option<CartId>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub collection_id: PaymentCollectionId,
    pub provider_code: String,
    pub amount: Money,
    pub context: Option<Value>,
    /// What the shopper chose to split the card into. Unchecked here on
    /// purpose: the bank decides what it will accept.
    pub installment_count: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct NewAccountHolder {
    pub provider_code: String,
    pub customer_id: Option<CustomerId>,
    pub external_id: String,
    pub email: Option<String>,
    pub data: Value,
}

/// What [`authorize`] settled on, which is not always a payment.
#[derive(Debug, Clone)]
pub enum Authorized {
    /// The provider is holding the money and a `payment` row now says so.
    Payment(Payment),
    /// The shopper has more to do. The session is open, nothing is canceled.
    RequiresMore(PaymentSession),
    /// The provider said no. The session is closed; a new one is the retry.
    Failed(PaymentSession),
}

impl Authorized {
    /// The payment, or an error saying what happened instead — for a caller
    /// that has already decided anything but a hold is a failure.
    pub fn payment(self) -> Result<Payment> {
        match self {
            Authorized::Payment(payment) => Ok(payment),
            Authorized::RequiresMore(_) => {
                Err(Error::conflict("that payment needs the shopper again"))
            }
            Authorized::Failed(_) => Err(Error::conflict("that payment was refused")),
        }
    }

    pub fn requires_more(&self) -> bool {
        matches!(self, Authorized::RequiresMore(_))
    }
}

// ---------------------------------------------------------------------------
// Providers
// ---------------------------------------------------------------------------

/// Which providers this shop has turned on. Configuration rather than money,
/// so it is judged as [`Resource::Pricing`] like the rest of the shop's setup.
pub async fn providers(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<Vec<Provider>> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    let rows = sqlx::query_as::<_, Provider>(
        "select id, code, is_enabled from payment_provider
         where scope = $1 order by code limit $2",
    )
    .bind(ctx.scope.0)
    .bind(MAX_PROVIDERS)
    .fetch_all(&mut **tx)
    .await?;

    Ok(rows)
}

pub async fn register_provider(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<Provider> {
    let _: Permit = ctx.permit(Action::Write, Resource::Pricing)?;

    let code = code.trim();
    if code.is_empty() {
        return Err(Error::invalid("a payment provider needs a code"));
    }

    let id = PaymentProviderId::new();
    let provider = sqlx::query_as::<_, Provider>(
        "insert into payment_provider (id, scope, code)
         values ($1, $2, $3)
         on conflict (scope, code) do update set is_enabled = payment_provider.is_enabled
         returning id, code, is_enabled",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(code)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "payment_provider",
            entity_id: provider.id.as_uuid(),
            summary: serde_json::json!({ "code": provider.code }),
        },
    )
    .await?;

    Ok(provider)
}

pub async fn provider_by_code(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<Provider> {
    let _: Permit = ctx.permit(Action::View, Resource::Pricing)?;

    sqlx::query_as::<_, Provider>(
        "select id, code, is_enabled from payment_provider where scope = $1 and code = $2",
    )
    .bind(ctx.scope.0)
    .bind(code)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("payment provider"))
}

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

pub async fn create_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewCollection,
) -> Result<PaymentCollection> {
    let id = PaymentCollectionId::new();
    let owner = match new.cart_id {
        Some(cart) => cart_owner(tx, ctx, cart).await?,
        None => None,
    };
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Payment {
            id: id.as_uuid(),
            order: Uuid::nil(),
            customer: owner,
        },
    )?;

    if new.amount.is_negative() {
        return Err(Error::invalid(
            "a payment collection cannot owe less than nothing",
        ));
    }

    let collection = sqlx::query_as::<_, PaymentCollection>(
        "insert into payment_collection (id, scope, cart_id, currency_code, amount, metadata)
         values ($1, $2, $3, $4, $5, $6)
         returning id, cart_id, currency_code, amount, authorized_amount, captured_amount,
                   refunded_amount, status, completed_at, created_at,
                   installment_count, surcharge_amount, merchant_surcharge_amount,
                   charged_amount",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.cart_id.map(CartId::as_uuid))
    .bind(new.amount.currency.as_str())
    .bind(new.amount.amount)
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "payment_collection",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "amount": new.amount.amount.to_string(),
                "currency": new.amount.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(collection)
}

pub async fn collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
) -> Result<PaymentCollection> {
    let owned = owned_payment_resource(tx, ctx, id).await?;
    let _: Permit = ctx.permit(Action::View, owned)?;
    read_collection(tx, ctx, id, false).await
}

/// Records that the provider moved an amount the shop did not ask for.
///
/// Not a [`Error::conflict`]: the money has already moved, so refusing the
/// write would leave a charged customer with a pending order. The collection
/// goes to `mismatch`, which nothing in this module clears, and the event is
/// what somebody is woken up by.
///
/// [`Action::Write`], not [`Action::Settle`]: writing down what a provider did
/// is bookkeeping, and a role that cannot move money must still be able to say
/// that money moved.
pub async fn flag_mismatch(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
    expected: Money,
    reported: Money,
) -> Result<PaymentCollection> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(id.as_uuid(), None))?;

    let collection = sqlx::query_as::<_, PaymentCollection>(
        "update payment_collection set status = 'mismatch'
         where scope = $1 and id = $2
         returning id, cart_id, currency_code, amount, authorized_amount, captured_amount,
                   refunded_amount, status, completed_at, created_at,
                   installment_count, surcharge_amount, merchant_surcharge_amount,
                   charged_amount",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("payment collection"))?;

    ctx.emit(
        tx,
        Event {
            name: "payment.amount_mismatch",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "expected": expected.amount.to_string(),
                "expected_currency": expected.currency.as_str(),
                "reported": reported.amount.to_string(),
                "reported_currency": reported.currency.as_str(),
            }),
        },
    )
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "payment_collection",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "status": "mismatch",
                "expected": expected.amount.to_string(),
                "reported": reported.amount.to_string(),
            }),
        },
    )
    .await?;

    Ok(collection)
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

/// Opens a session row before the provider is called, so the id handed to
/// [`PaymentProvider::create_session`] is one a webhook can be matched back to.
pub async fn create_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewSession,
) -> Result<PaymentSession> {
    let owned = owned_payment_resource(tx, ctx, new.collection_id).await?;
    let _: Permit = ctx.permit(Action::Write, owned)?;

    if new.amount.is_negative() {
        return Err(Error::invalid(
            "a payment session cannot be for less than nothing",
        ));
    }

    let provider = provider_row(tx, ctx, &new.provider_code).await?;
    if !provider.is_enabled {
        return Err(Error::invalid(format!(
            "the {} provider is turned off",
            provider.code
        )));
    }

    let collection = read_collection(tx, ctx, new.collection_id, false).await?;
    if collection.currency_code != new.amount.currency.as_str() {
        return Err(Error::invalid(
            "a session must be in the collection's currency",
        ));
    }

    let id = PaymentSessionId::new();
    let session = sqlx::query_as::<_, PaymentSession>(
        "insert into payment_session (
             id, scope, payment_collection_id, payment_provider_id,
             currency_code, amount, context, installment_count
         )
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, payment_collection_id, payment_provider_id, currency_code,
                   amount, status, data, context, authorized_at, created_at,
                   installment_count",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(new.collection_id.as_uuid())
    .bind(provider.id.as_uuid())
    .bind(new.amount.currency.as_str())
    .bind(new.amount.amount)
    .bind(new.context)
    .bind(new.installment_count)
    .fetch_one(&mut **tx)
    .await?;

    recompute(tx, ctx, new.collection_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "payment_session",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "provider": provider.code,
                "amount": new.amount.amount.to_string(),
            }),
        },
    )
    .await?;

    Ok(session)
}

/// Writes back what [`PaymentProvider::create_session`] answered.
pub async fn record_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
    response: SessionResponse,
) -> Result<PaymentSession> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(id.as_uuid(), None))?;

    if !response.status.is_open() {
        return Err(Error::invalid(
            "a session that has just been created is pending or requires more",
        ));
    }

    set_session_status(tx, ctx, id, response.status, Some(response.data), None).await
}

pub async fn session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
) -> Result<PaymentSession> {
    let _: Permit = ctx.permit(Action::View, payment_resource(id.as_uuid(), None))?;
    read_session(tx, ctx, id, false).await
}

pub async fn sessions(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
    paging: Paging,
) -> Result<Page<PaymentSession>> {
    let _: Permit = ctx.permit(Action::View, payment_resource(id.as_uuid(), None))?;

    let rows = sqlx::query_as::<_, PaymentSession>(
        "select id, payment_collection_id, payment_provider_id, currency_code,
                amount, status, data, context, authorized_at, created_at, installment_count
         from payment_session
         where scope = $1
           and payment_collection_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
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

// ---------------------------------------------------------------------------
// Authorise
// ---------------------------------------------------------------------------

/// Writes down what [`PaymentProvider::authorize`] answered.
///
/// [`Action::Write`] rather than [`Action::Settle`]: an authorisation is a hold,
/// and taking the money is [`capture`], which asks separately.
///
/// Authorising the same session twice returns the payment the first call wrote
/// rather than a second one — a webhook and a browser redirect routinely both
/// arrive.
pub async fn authorize(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
    auth: Authorization,
) -> Result<Authorized> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(id.as_uuid(), None))?;

    let session = read_session(tx, ctx, id, true).await?;

    if let Some(existing) = payment_for_session(tx, ctx, id).await? {
        return Ok(Authorized::Payment(existing));
    }

    match auth.status {
        AuthorizationStatus::RequiresMore => {
            let session = set_session_status(
                tx,
                ctx,
                id,
                SessionStatus::RequiresMore,
                Some(auth.data),
                None,
            )
            .await?;
            ctx.emit(
                tx,
                Event {
                    name: "payment.requires_more",
                    entity_id: id.as_uuid(),
                    payload: serde_json::json!({ "redirect": auth.redirect }),
                },
            )
            .await?;
            Ok(Authorized::RequiresMore(session))
        }
        AuthorizationStatus::Error => {
            let session =
                set_session_status(tx, ctx, id, SessionStatus::Error, Some(auth.data), None)
                    .await?;
            recompute(tx, ctx, session.payment_collection_id).await?;
            ctx.emit(
                tx,
                Event {
                    name: "payment.failed",
                    entity_id: id.as_uuid(),
                    payload: serde_json::json!({ "message": auth.message }),
                },
            )
            .await?;
            Ok(Authorized::Failed(session))
        }
        AuthorizationStatus::Authorized => {
            let asked = session.money()?;
            let held = auth.amount.unwrap_or(asked);
            if held.currency != asked.currency {
                return Err(Error::bug("a provider authorised another currency"));
            }

            let plan = auth.installment.clone();
            if let Some(plan) = &plan {
                if plan.surcharge.currency != asked.currency {
                    return Err(Error::bug("an instalment difference met another currency"));
                }
                if plan.count < 1 {
                    return Err(Error::invalid(
                        "an instalment plan is for one payment or more",
                    ));
                }
                if plan.surcharge.amount.is_sign_negative() {
                    return Err(Error::invalid("an instalment difference is not a discount"));
                }
            }

            // The shopper carries the difference on top of the basket; the shop
            // carries it out of the settlement and the card still sees the
            // basket. Only the first changes what may be authorised.
            let surcharge = match &plan {
                Some(plan) if plan.bearer == SurchargeBearer::Customer => plan.surcharge.amount,
                _ => Decimal::ZERO,
            };
            let agreed = Money::new(asked.amount + surcharge, asked.currency);

            let now = ctx.now();
            let payment_id = PaymentId::new();
            let payment = sqlx::query_as::<_, Payment>(
                "insert into payment (
                     id, scope, payment_collection_id, payment_session_id,
                     payment_provider_id, currency_code, amount, data,
                     installment_count, surcharge_amount, surcharge_bearer, installment_campaign
                 )
                 values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
                 returning id, payment_collection_id, payment_session_id, payment_provider_id,
                           currency_code, amount, data, captured_at, canceled_at, created_at,
                           installment_count, surcharge_amount, surcharge_bearer,
                           installment_campaign",
            )
            .bind(payment_id.as_uuid())
            .bind(ctx.scope.0)
            .bind(session.payment_collection_id.as_uuid())
            .bind(id.as_uuid())
            .bind(session.payment_provider_id.as_uuid())
            .bind(held.currency.as_str())
            .bind(held.amount)
            .bind(auth.data.clone())
            .bind(plan.as_ref().map(|p| p.count))
            .bind(plan.as_ref().map_or(Decimal::ZERO, |p| p.surcharge.amount))
            .bind(plan.as_ref().map(|p| p.bearer.as_str()))
            .bind(plan.as_ref().and_then(|p| p.campaign.clone()))
            .fetch_one(&mut **tx)
            .await?;

            set_session_status(
                tx,
                ctx,
                id,
                SessionStatus::Authorized,
                Some(auth.data),
                Some(now),
            )
            .await?;

            recompute(tx, ctx, session.payment_collection_id).await?;

            // An instalment sale is authorised for more than the order came to
            // by agreement, so the guard asks whether the difference is
            // explained rather than whether there is one. An unexplained
            // difference is still a mismatch.
            if held.amount != agreed.amount {
                flag_mismatch(tx, ctx, session.payment_collection_id, agreed, held).await?;
            }

            ctx.audit(
                tx,
                AuditEntry {
                    actor: ctx.actor.clone(),
                    action: Action::Write,
                    entity: "payment",
                    entity_id: payment_id.as_uuid(),
                    summary: serde_json::json!({
                        "session": id.to_string(),
                        "amount": held.amount.to_string(),
                    }),
                },
            )
            .await?;

            ctx.emit(
                tx,
                Event {
                    name: "payment.authorized",
                    entity_id: payment_id.as_uuid(),
                    payload: serde_json::json!({
                        "collection": session.payment_collection_id.to_string(),
                        "amount": held.amount.to_string(),
                        "currency": held.currency.as_str(),
                    }),
                },
            )
            .await?;

            Ok(Authorized::Payment(payment))
        }
    }
}

// ---------------------------------------------------------------------------
// Capture and refund
// ---------------------------------------------------------------------------

/// Records money actually taken, in part or in whole.
///
/// [`Action::Settle`], asked separately from the authorisation that made it
/// possible.
///
/// **There is no compensation for this.** A workflow step that captures does
/// not undo itself when a later step fails; the money is out of the cardholder's
/// account and the only way back is [`refund`], which is another provider call
/// and another row.
pub async fn capture(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    amount: Money,
    metadata: Option<Value>,
) -> Result<Capture> {
    let _: Permit = ctx.permit(Action::Settle, payment_resource(id.as_uuid(), None))?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a capture is for more than nothing"));
    }

    let payment = read_payment(tx, ctx, id, true).await?;
    if payment.canceled_at.is_some() {
        return Err(Error::conflict("that payment was canceled"));
    }
    if payment.currency_code != amount.currency.as_str() {
        return Err(Error::bug("a capture met another currency"));
    }

    let balance = balance_of(tx, ctx, id).await?;
    if amount.amount > balance.capturable() {
        return Err(Error::conflict(format!(
            "capturing {} leaves only {} authorised",
            amount.amount,
            balance.capturable()
        )));
    }

    let capture_id = CaptureId::new();
    let capture = sqlx::query_as::<_, Capture>(
        "insert into capture (id, scope, payment_id, amount, currency_code, created_by, metadata)
         values ($1, $2, $3, $4, $5, $6, $7)
         returning id, payment_id, amount, currency_code, created_by, created_at",
    )
    .bind(capture_id.as_uuid())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(who(&ctx.actor))
    .bind(metadata)
    .fetch_one(&mut **tx)
    .await?;

    let taken = balance.captured + amount.amount;
    sqlx::query(
        "update payment set captured_at = $3
         where scope = $1 and id = $2 and captured_at is null and $4 >= amount",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .bind(taken)
    .execute(&mut **tx)
    .await?;

    recompute(tx, ctx, payment.payment_collection_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "capture",
            entity_id: capture_id.as_uuid(),
            summary: serde_json::json!({
                "payment": id.to_string(),
                "amount": amount.amount.to_string(),
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "payment.captured",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "capture": capture_id.to_string(),
                "amount": amount.amount.to_string(),
                "currency": amount.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(capture)
}

/// Gives back money that was taken. Never more than was taken, and the check is
/// made with the payment row locked because two refunds always turn up at once.
pub async fn refund(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    amount: Money,
    reason: Option<RefundReasonId>,
    note: Option<String>,
) -> Result<Refund> {
    let _: Permit = ctx.permit(Action::Settle, payment_resource(id.as_uuid(), None))?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a refund is for more than nothing"));
    }

    let payment = read_payment(tx, ctx, id, true).await?;
    if payment.currency_code != amount.currency.as_str() {
        return Err(Error::bug("a refund met another currency"));
    }

    let balance = balance_of(tx, ctx, id).await?;
    if amount.amount > balance.refundable() {
        return Err(Error::conflict(format!(
            "refunding {} of {} that was captured and not yet given back",
            amount.amount,
            balance.refundable()
        )));
    }

    let refund_id = RefundId::new();
    let refund = sqlx::query_as::<_, Refund>(
        "insert into refund (
             id, scope, payment_id, refund_reason_id, amount, currency_code, note, created_by
         )
         values ($1, $2, $3, $4, $5, $6, $7, $8)
         returning id, payment_id, refund_reason_id, amount, currency_code, note,
                   created_by, created_at",
    )
    .bind(refund_id.as_uuid())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(reason.map(|r| r.as_uuid()))
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(note)
    .bind(who(&ctx.actor))
    .fetch_one(&mut **tx)
    .await?;

    recompute(tx, ctx, payment.payment_collection_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "refund",
            entity_id: refund_id.as_uuid(),
            summary: serde_json::json!({
                "payment": id.to_string(),
                "amount": amount.amount.to_string(),
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "payment.refunded",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "refund": refund_id.to_string(),
                "amount": amount.amount.to_string(),
                "currency": amount.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(refund)
}

/// Releases an authorisation nothing was taken against.
pub async fn cancel(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<Payment> {
    let _: Permit = ctx.permit(Action::Settle, payment_resource(id.as_uuid(), None))?;

    let payment = read_payment(tx, ctx, id, true).await?;
    let balance = balance_of(tx, ctx, id).await?;
    if balance.captured > Decimal::ZERO {
        return Err(Error::conflict(
            "money has been taken against that payment; refund it instead",
        ));
    }

    let canceled = sqlx::query_as::<_, Payment>(
        "update payment set canceled_at = coalesce(canceled_at, $3)
         where scope = $1 and id = $2
         returning id, payment_collection_id, payment_session_id, payment_provider_id,
                   currency_code, amount, data, captured_at, canceled_at, created_at,
                   installment_count, surcharge_amount, surcharge_bearer, installment_campaign",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .fetch_one(&mut **tx)
    .await?;

    if let Some(session) = payment.payment_session_id {
        set_session_status(tx, ctx, session, SessionStatus::Canceled, None, None).await?;
    }

    recompute(tx, ctx, payment.payment_collection_id).await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "payment",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "canceled": true }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "payment.canceled",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({}),
        },
    )
    .await?;

    Ok(canceled)
}

pub async fn payment(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<Payment> {
    let _: Permit = ctx.permit(Action::View, payment_resource(id.as_uuid(), None))?;
    read_payment(tx, ctx, id, false).await
}

/// What is still capturable and still refundable, summed from the rows rather
/// than read off a counter.
pub async fn balance(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<Balance> {
    let _: Permit = ctx.permit(Action::View, payment_resource(id.as_uuid(), None))?;
    balance_of(tx, ctx, id).await
}

// ---------------------------------------------------------------------------
// Account holders
// ---------------------------------------------------------------------------

/// The customer as one provider knows them. Written again with the same
/// `external_id` updates rather than duplicates: a saved card must not fork.
pub async fn save_account_holder(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    new: NewAccountHolder,
) -> Result<AccountHolder> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Customer {
            id: new.customer_id.map(|c| c.as_uuid()),
        },
    )?;

    if new.external_id.trim().is_empty() {
        return Err(Error::invalid("an account holder needs the provider's id"));
    }

    let provider = provider_row(tx, ctx, &new.provider_code).await?;

    let holder = sqlx::query_as::<_, AccountHolder>(
        "insert into account_holder (
             id, scope, payment_provider_id, customer_id, external_id, email, data
         )
         values ($1, $2, $3, $4, $5, $6, $7)
         on conflict (scope, payment_provider_id, external_id) do update
             set customer_id = excluded.customer_id,
                 email = excluded.email,
                 data = excluded.data
         returning id, payment_provider_id, customer_id, external_id, email, data, created_at",
    )
    .bind(AccountHolderId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(provider.id.as_uuid())
    .bind(new.customer_id.map(|c| c.as_uuid()))
    .bind(new.external_id.trim())
    .bind(new.email)
    .bind(new.data)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "account_holder",
            entity_id: holder.id.as_uuid(),
            summary: serde_json::json!({ "provider": provider.code }),
        },
    )
    .await?;

    Ok(holder)
}

pub async fn account_holder(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    provider_code: &str,
    customer: CustomerId,
) -> Result<Option<AccountHolder>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer.as_uuid()),
        },
    )?;

    let provider = provider_row(tx, ctx, provider_code).await?;

    let holder = sqlx::query_as::<_, AccountHolder>(
        "select id, payment_provider_id, customer_id, external_id, email, data, created_at
         from account_holder
         where scope = $1 and payment_provider_id = $2 and customer_id = $3
         order by created_at desc
         limit 1",
    )
    .bind(ctx.scope.0)
    .bind(provider.id.as_uuid())
    .bind(customer.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(holder)
}

// ---------------------------------------------------------------------------
// Webhooks
// ---------------------------------------------------------------------------

/// Writes the event down before anything acts on it.
///
/// The insert is `on conflict do nothing` against the unique
/// `(scope, provider, event_id)`, so a redelivery affects no rows and comes back
/// [`WebhookOutcome::AlreadySeen`]. The caller does nothing on that answer —
/// no capture, no status change, no event — and acknowledges the provider.
pub async fn record_webhook(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    provider_code: &str,
    event: &WebhookEvent,
) -> Result<WebhookOutcome> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(Uuid::nil(), None))?;

    if event.event_id.trim().is_empty() {
        return Err(Error::invalid("a webhook event needs the provider's id"));
    }

    let provider = provider_row(tx, ctx, provider_code).await?;

    let id = PaymentWebhookEventId::new();
    let written = sqlx::query(
        "insert into payment_webhook_event (
             id, scope, payment_provider_id, event_id, event_type, payload
         )
         values ($1, $2, $3, $4, $5, $6)
         on conflict (scope, payment_provider_id, event_id) do nothing",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(provider.id.as_uuid())
    .bind(event.event_id.trim())
    .bind(event.event_type.as_str())
    .bind(event.payload.clone())
    .execute(&mut **tx)
    .await?;

    if written.rows_affected() == 0 {
        return Ok(WebhookOutcome::AlreadySeen);
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "payment_webhook_event",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "provider": provider.code,
                "event_type": event.event_type,
            }),
        },
    )
    .await?;

    Ok(WebhookOutcome::Fresh { id })
}

/// Says the event has been acted on. Until this lands the event is still work,
/// which is what makes a crash between the two resumable.
pub async fn mark_processed(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentWebhookEventId,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(id.as_uuid(), None))?;

    let done = sqlx::query(
        "update payment_webhook_event
         set processed_at = coalesce(processed_at, $3), last_error = null
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if done.rows_affected() == 0 {
        return Err(Error::not_found("payment webhook event"));
    }

    Ok(())
}

/// Leaves the event unprocessed and counts the attempt, so [`unprocessed`]
/// hands it back. An event that arrived before the thing it refers to lives
/// here until it can be applied.
pub async fn mark_failed(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentWebhookEventId,
    error: &str,
) -> Result<()> {
    let _: Permit = ctx.permit(Action::Write, payment_resource(id.as_uuid(), None))?;

    let noted = sqlx::query(
        "update payment_webhook_event
         set attempts = attempts + 1, last_error = $3
         where scope = $1 and id = $2 and processed_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(error)
    .execute(&mut **tx)
    .await?;

    if noted.rows_affected() == 0 {
        return Err(Error::not_found("payment webhook event"));
    }

    Ok(())
}

/// Events received and not yet acted on, oldest first.
pub async fn unprocessed(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    paging: Paging,
) -> Result<Page<PendingWebhook>> {
    let _: Permit = ctx.permit(Action::View, payment_resource(Uuid::nil(), None))?;

    let rows = sqlx::query_as::<_, PendingWebhook>(
        "select id, payment_provider_id, event_id, event_type, payload, attempts, received_at
         from payment_webhook_event
         where scope = $1
           and processed_at is null
           and ($2::timestamptz is null or (received_at, id) > ($2, $3))
         order by received_at, id
         limit $4",
    )
    .bind(ctx.scope.0)
    .bind(paging.after.map(|c| c.at))
    .bind(paging.after.map(|c| c.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.received_at,
        id: row.id.as_uuid(),
    }))
}

// ---------------------------------------------------------------------------
// Inside
// ---------------------------------------------------------------------------

/// A collection is not always attached to an order yet — one is opened before
/// the order exists — so a nil uuid is what "no order to judge by" looks like.
fn payment_resource(id: Uuid, order: Option<Uuid>) -> Resource {
    Resource::Payment {
        id,
        order: order.unwrap_or_else(Uuid::nil),
        customer: None,
    }
}

/// The same, with the owner read off whatever the collection is attached to,
/// so an authorizer is asked a question it can answer.
///
/// The lookup takes no permission of its own: it answers nothing to the
/// caller, and the permission is taken with what it found.
async fn owned_payment_resource(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
) -> Result<Resource> {
    let found: Option<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"select o.id, coalesce(o.customer_id, c.customer_id)
           from payment_collection pc
           left join cart c on c.scope = pc.scope and c.id = pc.cart_id
           left join "order" o on o.scope = pc.scope and o.payment_collection_id = pc.id
           where pc.scope = $1 and pc.id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let (order, customer) = found.unwrap_or((None, None));

    Ok(Resource::Payment {
        id: id.as_uuid(),
        order: order.unwrap_or_else(Uuid::nil),
        customer,
    })
}

/// The owner of a cart, for a collection being opened against it.
async fn cart_owner(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart: CartId) -> Result<Option<Uuid>> {
    Ok(sqlx::query_scalar::<_, Option<Uuid>>(
        "select customer_id from cart where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(cart.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .flatten())
}

fn who(actor: &Actor) -> String {
    match actor {
        Actor::Staff { id } => format!("staff:{id}"),
        Actor::Customer { id } => format!("customer:{id}"),
        Actor::Guest { cart } => format!("guest:{cart}"),
        Actor::System => "system".to_owned(),
    }
}

async fn provider_row(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<Provider> {
    sqlx::query_as::<_, Provider>(
        "select id, code, is_enabled from payment_provider where scope = $1 and code = $2",
    )
    .bind(ctx.scope.0)
    .bind(code)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("payment provider"))
}

async fn read_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
    lock: bool,
) -> Result<PaymentCollection> {
    let sql = if lock {
        "select id, cart_id, currency_code, amount, authorized_amount, captured_amount,
                refunded_amount, status, completed_at, created_at,
                installment_count, surcharge_amount, merchant_surcharge_amount, charged_amount
         from payment_collection where scope = $1 and id = $2 for update"
    } else {
        "select id, cart_id, currency_code, amount, authorized_amount, captured_amount,
                refunded_amount, status, completed_at, created_at,
                installment_count, surcharge_amount, merchant_surcharge_amount, charged_amount
         from payment_collection where scope = $1 and id = $2"
    };

    sqlx::query_as::<_, PaymentCollection>(sql)
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found("payment collection"))
}

async fn read_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
    lock: bool,
) -> Result<PaymentSession> {
    let sql = if lock {
        "select id, payment_collection_id, payment_provider_id, currency_code,
                amount, status, data, context, authorized_at, created_at, installment_count
         from payment_session where scope = $1 and id = $2 for update"
    } else {
        "select id, payment_collection_id, payment_provider_id, currency_code,
                amount, status, data, context, authorized_at, created_at, installment_count
         from payment_session where scope = $1 and id = $2"
    };

    sqlx::query_as::<_, PaymentSession>(sql)
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found("payment session"))
}

async fn read_payment(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentId,
    lock: bool,
) -> Result<Payment> {
    let sql = if lock {
        "select id, payment_collection_id, payment_session_id, payment_provider_id,
                currency_code, amount, data, captured_at, canceled_at, created_at,
                installment_count, surcharge_amount, surcharge_bearer, installment_campaign
         from payment where scope = $1 and id = $2 for update"
    } else {
        "select id, payment_collection_id, payment_session_id, payment_provider_id,
                currency_code, amount, data, captured_at, canceled_at, created_at,
                installment_count, surcharge_amount, surcharge_bearer, installment_campaign
         from payment where scope = $1 and id = $2"
    };

    sqlx::query_as::<_, Payment>(sql)
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found("payment"))
}

async fn payment_for_session(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
) -> Result<Option<Payment>> {
    let found = sqlx::query_as::<_, Payment>(
        "select id, payment_collection_id, payment_session_id, payment_provider_id,
                currency_code, amount, data, captured_at, canceled_at, created_at,
                installment_count, surcharge_amount, surcharge_bearer, installment_campaign
         from payment
         where scope = $1 and payment_session_id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    Ok(found)
}

async fn set_session_status(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentSessionId,
    status: SessionStatus,
    data: Option<Value>,
    authorized_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<PaymentSession> {
    sqlx::query_as::<_, PaymentSession>(
        "update payment_session
         set status = $3,
             data = coalesce($4::jsonb, data),
             authorized_at = coalesce($5::timestamptz, authorized_at)
         where scope = $1 and id = $2
         returning id, payment_collection_id, payment_provider_id, currency_code,
                   amount, status, data, context, authorized_at, created_at,
                   installment_count",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(status.as_str())
    .bind(data)
    .bind(authorized_at)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("payment session"))
}

async fn balance_of(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<Balance> {
    let row: (Decimal, Decimal, Decimal) = sqlx::query_as(
        "select p.amount,
                coalesce((select sum(c.amount) from capture c
                          where c.scope = $1 and c.payment_id = p.id), 0),
                coalesce((select sum(r.amount) from refund r
                          where r.scope = $1 and r.payment_id = p.id), 0)
         from payment p
         where p.scope = $1 and p.id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("payment"))?;

    Ok(Balance {
        authorized: row.0,
        captured: row.1,
        refunded: row.2,
    })
}

/// Sums the collection's three amounts from the rows and names the state they
/// add up to. `mismatch` is left alone: it is a person's to clear, and a later
/// capture must not quietly agree with the provider on the shop's behalf.
async fn recompute(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: PaymentCollectionId,
) -> Result<PaymentCollection> {
    let collection = read_collection(tx, ctx, id, true).await?;

    let plan: (Decimal, Decimal, Option<i32>) = sqlx::query_as(
        "select
             coalesce(sum(p.surcharge_amount) filter (where p.surcharge_bearer = 'customer'), 0),
             coalesce(sum(p.surcharge_amount) filter (where p.surcharge_bearer = 'merchant'), 0),
             coalesce(max(p.installment_count),
                      (select max(s.installment_count) from payment_session s
                       where s.scope = $1 and s.payment_collection_id = $2))
         from payment p
         where p.scope = $1 and p.payment_collection_id = $2 and p.canceled_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    let (customer_surcharge, merchant_surcharge, installments) = plan;
    let charged = collection.amount + customer_surcharge;

    let sums: (Decimal, Decimal, Decimal, i64, i64) = sqlx::query_as(
        "select
             coalesce((select sum(p.amount) from payment p
                       where p.scope = $1 and p.payment_collection_id = $2
                         and p.canceled_at is null), 0),
             coalesce((select sum(c.amount) from capture c
                       join payment p on p.id = c.payment_id and p.scope = c.scope
                       where c.scope = $1 and p.payment_collection_id = $2), 0),
             coalesce((select sum(r.amount) from refund r
                       join payment p on p.id = r.payment_id and p.scope = r.scope
                       where r.scope = $1 and p.payment_collection_id = $2), 0),
             (select count(*) from payment_session s
              where s.scope = $1 and s.payment_collection_id = $2),
             (select count(*) from payment p
              where p.scope = $1 and p.payment_collection_id = $2
                and p.canceled_at is not null)",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    let (authorized, captured, refunded, sessions, canceled) = sums;

    let status = if collection.status() == CollectionStatus::Mismatch {
        CollectionStatus::Mismatch
    } else if refunded > Decimal::ZERO && refunded >= captured {
        CollectionStatus::Refunded
    } else if refunded > Decimal::ZERO {
        CollectionStatus::PartiallyRefunded
    } else if captured >= charged && captured > Decimal::ZERO {
        CollectionStatus::Captured
    } else if captured > Decimal::ZERO {
        CollectionStatus::PartiallyCaptured
    } else if authorized >= charged && authorized > Decimal::ZERO {
        CollectionStatus::Authorized
    } else if authorized > Decimal::ZERO {
        CollectionStatus::PartiallyAuthorized
    } else if canceled > 0 {
        CollectionStatus::Canceled
    } else if sessions > 0 {
        CollectionStatus::Awaiting
    } else {
        CollectionStatus::NotPaid
    };

    let done = matches!(
        status,
        CollectionStatus::Captured | CollectionStatus::Refunded | CollectionStatus::Canceled
    );

    let updated = sqlx::query_as::<_, PaymentCollection>(
        "update payment_collection
         set authorized_amount = $3,
             captured_amount = $4,
             refunded_amount = $5,
             status = $6,
             completed_at = case when $7 then coalesce(completed_at, $8) else null end,
             surcharge_amount = $9,
             merchant_surcharge_amount = $10,
             charged_amount = $11,
             installment_count = $12
         where scope = $1 and id = $2
         returning id, cart_id, currency_code, amount, authorized_amount, captured_amount,
                   refunded_amount, status, completed_at, created_at,
                   installment_count, surcharge_amount, merchant_surcharge_amount,
                   charged_amount",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(authorized)
    .bind(captured)
    .bind(refunded)
    .bind(status.as_str())
    .bind(done)
    .bind(ctx.now())
    .bind(customer_surcharge)
    .bind(merchant_surcharge)
    .bind(charged)
    .bind(installments)
    .fetch_one(&mut **tx)
    .await?;

    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn a_status_survives_the_round_trip_through_the_column() {
        for status in [
            SessionStatus::Pending,
            SessionStatus::RequiresMore,
            SessionStatus::Authorized,
            SessionStatus::Captured,
            SessionStatus::Canceled,
            SessionStatus::Error,
        ] {
            assert_eq!(SessionStatus::parse(status.as_str()), status);
        }
    }

    #[test]
    fn a_collection_status_survives_the_round_trip_through_the_column() {
        for status in [
            CollectionStatus::NotPaid,
            CollectionStatus::Awaiting,
            CollectionStatus::PartiallyAuthorized,
            CollectionStatus::Authorized,
            CollectionStatus::PartiallyCaptured,
            CollectionStatus::Captured,
            CollectionStatus::PartiallyRefunded,
            CollectionStatus::Refunded,
            CollectionStatus::Canceled,
            CollectionStatus::Failed,
            CollectionStatus::Mismatch,
        ] {
            assert_eq!(CollectionStatus::parse(status.as_str()), status);
        }
    }

    #[test]
    fn nothing_is_refundable_before_it_is_captured() {
        let held = Balance {
            authorized: dec!(100),
            captured: dec!(0),
            refunded: dec!(0),
        };
        assert_eq!(held.refundable(), dec!(0));
        assert_eq!(held.capturable(), dec!(100));
    }

    #[test]
    fn a_refund_eats_into_what_may_still_be_given_back() {
        let taken = Balance {
            authorized: dec!(100),
            captured: dec!(60),
            refunded: dec!(25),
        };
        assert_eq!(taken.refundable(), dec!(35));
        assert_eq!(taken.capturable(), dec!(40));
    }
}
