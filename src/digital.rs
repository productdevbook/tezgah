//! Digital products: a key, a right to it, and a count of what was taken.
//!
//! Three objects and no more. A variant carries [`DigitalContent`] — what the
//! host's storage calls the file, and nothing else about it. Paying for a line
//! that carries one freezes an [`OrderEntitlement`] against that line. Every
//! download writes a row of `entitlement_access`, which is the table a
//! chargeback is answered from.
//!
//! # What this deliberately does not do
//!
//! tezgah never streams a byte, signs a URL or talks to object storage. It
//! stores a key and a count, and [`redeem`] answers "yes, and here is the key,
//! and that was the fourth of five" — turning that into bytes is the host's,
//! because the host is the one that knows where the bytes are.
//!
//! # Frozen at the grant
//!
//! `content_key`, `max_downloads` and `expires_at` are copied onto the
//! entitlement when it is granted, the way `order_line_item` copies the title.
//! Changing the file on the variant next year must not change what somebody
//! already bought.
//!
//! # Granting is not checkout
//!
//! Checkout authorises; it does not take the money. Handing over a file
//! against a hold is giving the goods away before the till rings. So [`grant`]
//! is called where money arrives — after a capture — and [`revoke`] where it
//! goes back, in the transaction that gives it back.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    CustomerId, DigitalContentId, EntitlementTokenId, LineItemId, OrderEntitlementId, OrderId,
    VariantId,
};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};

/// Most files one variant may carry. A book in every format anybody reads is a
/// handful; a thousand is an import that went wrong.
pub const MAX_CONTENT_PER_VARIANT: i64 = 50;

/// Most entitlements one order may hold. Also what bounds [`revoke`], which is
/// why granting refuses beyond it rather than truncating quietly.
pub const MAX_ENTITLEMENTS_PER_ORDER: i64 = 500;

/// How long a download link lives. Short on purpose: it is a bearer thing that
/// travels in an e-mail, and the entitlement behind it outlives it.
pub const TOKEN_MINUTES: i64 = 60;

const CONTENT_COLUMNS: &str = "id, variant_id, content_key, name, max_downloads, valid_days, \
                               auto_grant, rank, metadata, deleted_at, created_at, updated_at";

const ENTITLEMENT_COLUMNS: &str = "id, order_id, order_line_item_id, digital_content_id, customer_id, content_key, \
     max_downloads, granted_at, expires_at, download_count, revoked_at, revoked_reason, created_at";

// ---------------------------------------------------------------------------
// What the rows look like
// ---------------------------------------------------------------------------

/// One file a variant carries.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DigitalContent {
    pub id: DigitalContentId,
    pub variant_id: VariantId,
    /// Whatever the host's storage calls it. tezgah never resolves it.
    pub content_key: String,
    pub name: String,
    pub max_downloads: Option<i32>,
    /// How long the buyer has, counted from the moment they paid.
    pub valid_days: Option<i32>,
    pub auto_grant: bool,
    pub rank: i32,
    pub metadata: Option<serde_json::Value>,
    pub deleted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct NewContent {
    pub content_key: String,
    pub name: String,
    pub max_downloads: Option<i32>,
    pub valid_days: Option<i32>,
    pub auto_grant: Option<bool>,
    pub rank: Option<i32>,
    pub metadata: Option<serde_json::Value>,
}

/// What one order line bought, as it stood on the day it was paid for.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderEntitlement {
    pub id: OrderEntitlementId,
    pub order_id: OrderId,
    pub order_line_item_id: LineItemId,
    pub digital_content_id: DigitalContentId,
    pub customer_id: Option<CustomerId>,
    pub content_key: String,
    pub max_downloads: Option<i32>,
    pub granted_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub download_count: i32,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked_reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl OrderEntitlement {
    /// How many downloads are left, when there is a limit at all.
    pub fn remaining(&self) -> Option<i32> {
        self.max_downloads
            .map(|most| (most - self.download_count).max(0))
    }

    pub fn is_live(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.revoked_at.is_none()
            && self.expires_at.is_none_or(|at| at > now)
            && self.remaining().is_none_or(|left| left > 0)
    }
}

/// A fresh link and the one time its token is readable. Only the hash is kept.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    pub id: EntitlementTokenId,
    pub token: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub entitlement: OrderEntitlement,
}

/// Who is downloading, for the row that says so afterwards.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// The signed-in shopper, when a storefront is asking. `None` is the back
    /// office, which is not limited to one customer's entitlements.
    pub as_customer: Option<CustomerId>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

/// What a caller hands to its storage to produce bytes.
#[derive(Debug, Clone)]
pub struct Download {
    pub entitlement_id: OrderEntitlementId,
    pub content_key: String,
    pub download_count: i32,
    pub remaining: Option<i32>,
}

// ---------------------------------------------------------------------------
// The catalogue side
// ---------------------------------------------------------------------------

/// Puts a file on a variant, or edits the one already under that key.
///
/// A catalogue write rather than an order one, so it asks about the product:
/// whoever may edit what is sold may say which file is sold with it.
pub async fn put_content(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
    new: NewContent,
) -> Result<DigitalContent> {
    let _: Permit = ctx.permit(Action::Write, Resource::Product { id: None })?;

    let key = new.content_key.trim();
    if key.is_empty() {
        return Err(Error::invalid("a digital file needs a key to find it by"));
    }
    let name = new.name.trim();
    if name.is_empty() {
        return Err(Error::invalid("a digital file needs a name"));
    }
    if new.max_downloads.is_some_and(|most| most <= 0) {
        return Err(Error::invalid("a file nobody may download is not a file"));
    }
    if new.valid_days.is_some_and(|days| days <= 0) {
        return Err(Error::invalid("a right that has already expired"));
    }

    let content = sqlx::query_as::<_, DigitalContent>(&format!(
        "insert into digital_content
             (id, scope, variant_id, content_key, name, max_downloads, valid_days,
              auto_grant, rank, metadata)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         on conflict (scope, variant_id, content_key) where deleted_at is null
         do update set name = excluded.name,
                       max_downloads = excluded.max_downloads,
                       valid_days = excluded.valid_days,
                       auto_grant = excluded.auto_grant,
                       rank = excluded.rank,
                       metadata = excluded.metadata
         returning {CONTENT_COLUMNS}"
    ))
    .bind(DigitalContentId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(key)
    .bind(name)
    .bind(new.max_downloads)
    .bind(new.valid_days)
    .bind(new.auto_grant.unwrap_or(true))
    .bind(new.rank.unwrap_or(0))
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "digital_content",
            entity_id: content.id.as_uuid(),
            summary: serde_json::json!({
                "variant": variant_id.to_string(),
                "name": content.name,
            }),
        },
    )
    .await?;

    Ok(content)
}

/// The files a variant carries, in the order a storefront should list them.
pub async fn contents(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    variant_id: VariantId,
) -> Result<Vec<DigitalContent>> {
    let _: Permit = ctx.permit(Action::View, Resource::Product { id: None })?;

    Ok(sqlx::query_as::<_, DigitalContent>(&format!(
        "select {CONTENT_COLUMNS} from digital_content
         where scope = $1 and variant_id = $2 and deleted_at is null
         order by rank, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(variant_id.as_uuid())
    .bind(MAX_CONTENT_PER_VARIANT)
    .fetch_all(&mut **tx)
    .await?)
}

/// Takes a file off a variant. Soft, because entitlements already granted key
/// to the row, and their copy of `content_key` is what they are honoured by.
pub async fn delete_content(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: DigitalContentId) -> Result<()> {
    let _: Permit = ctx.permit(Action::Delete, Resource::Product { id: None })?;

    let gone = sqlx::query(
        "update digital_content set deleted_at = $3
         where scope = $1 and id = $2 and deleted_at is null",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .execute(&mut **tx)
    .await?;

    if gone.rows_affected() == 0 {
        return Err(Error::not_found("digital content"));
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "digital_content",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({}),
        },
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// The order side
// ---------------------------------------------------------------------------

#[derive(FromRow)]
struct GrantableRow {
    content_id: Uuid,
    content_key: String,
    max_downloads: Option<i32>,
    valid_days: Option<i32>,
    line_item_id: Uuid,
    unit_price: Decimal,
    quantity: i32,
    customer_id: Option<Uuid>,
}

/// Grants what an order's digital lines bought, once the shop has the money.
///
/// Safe to call again, which is the point: a provider redelivers its webhook
/// and the second delivery finds every line already granted and grants
/// nothing. `order_entitlement_line_content_key` is what makes that true
/// against two deliveries arriving at once rather than one after the other.
///
/// Nothing is granted while the money is only authorised. A partial capture
/// below what the digital lines are worth has not bought them, and the next
/// capture finds them still ungranted.
pub async fn grant(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderEntitlement>> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, GrantableRow>(
        r#"select d.id as content_id, d.content_key, d.max_downloads, d.valid_days,
                  l.id as line_item_id, l.unit_price, i.quantity, o.customer_id
           from order_line_item l
           join "order" o on o.scope = l.scope and o.id = l.order_id
           join order_item i
             on i.scope = l.scope and i.order_line_item_id = l.id and i.version = o.version
           join digital_content d
             on d.scope = l.scope and d.variant_id = l.variant_id
                and d.deleted_at is null and d.auto_grant
           where l.scope = $1 and l.order_id = $2 and i.quantity > 0
           order by l.created_at, l.id, d.rank, d.id
           limit $3"#,
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_ENTITLEMENTS_PER_ORDER)
    .fetch_all(&mut **tx)
    .await?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    // What the digital half of the order is worth: a line carrying three files
    // is one line's money, not three.
    let mut lines: Vec<(Uuid, Decimal)> = Vec::new();
    for row in &rows {
        if !lines.iter().any(|(id, _)| *id == row.line_item_id) {
            lines.push((
                row.line_item_id,
                row.unit_price * Decimal::from(row.quantity),
            ));
        }
    }
    let owed: Decimal = lines.iter().map(|(_, amount)| *amount).sum();

    // Only money the shop is holding: a `payment` row is an authorisation, and
    // a hold is not a payment.
    let taken: Decimal = sqlx::query_scalar(
        "select coalesce(sum(amount), 0) from order_transaction
         where scope = $1 and order_id = $2
           and reference in ('capture', 'refund', 'credit_line')",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    if taken < owed {
        return Ok(Vec::new());
    }

    let now = ctx.now();
    let mut granted = Vec::new();

    for row in &rows {
        let expires_at = row
            .valid_days
            .and_then(|days| chrono::Duration::try_days(i64::from(days)))
            .map(|span| now + span);

        let entitlement = sqlx::query_as::<_, OrderEntitlement>(&format!(
            "insert into order_entitlement
                 (id, scope, order_id, order_line_item_id, digital_content_id, customer_id,
                  content_key, max_downloads, granted_at, expires_at)
             values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
             on conflict (scope, order_line_item_id, digital_content_id) do nothing
             returning {ENTITLEMENT_COLUMNS}"
        ))
        .bind(OrderEntitlementId::new().as_uuid())
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .bind(row.line_item_id)
        .bind(row.content_id)
        .bind(row.customer_id)
        .bind(&row.content_key)
        .bind(row.max_downloads)
        .bind(now)
        .bind(expires_at)
        .fetch_optional(&mut **tx)
        .await?;

        let Some(entitlement) = entitlement else {
            continue;
        };

        ctx.audit(
            tx,
            AuditEntry {
                actor: ctx.actor.clone(),
                action: Action::Write,
                entity: "order_entitlement",
                entity_id: entitlement.id.as_uuid(),
                summary: serde_json::json!({
                    "order": order_id.to_string(),
                    "line": entitlement.order_line_item_id.to_string(),
                }),
            },
        )
        .await?;

        ctx.emit(
            tx,
            Event {
                name: "entitlement.granted",
                entity_id: entitlement.id.as_uuid(),
                payload: serde_json::json!({
                    "order": order_id,
                    "customer": entitlement.customer_id,
                    "expires_at": entitlement.expires_at,
                }),
            },
        )
        .await?;

        granted.push(entitlement);
    }

    Ok(granted)
}

/// Takes back what an order's money paid for, in the transaction that gives
/// the money back. This is the line every hand-rolled version forgets.
pub async fn revoke(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    reason: Option<&str>,
) -> Result<Vec<OrderEntitlement>> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    let revoked = sqlx::query_as::<_, OrderEntitlement>(&format!(
        "update order_entitlement set revoked_at = $3, revoked_reason = $4
         where scope = $1 and revoked_at is null and id in (
             select id from order_entitlement
             where scope = $1 and order_id = $2 and revoked_at is null
             order by created_at, id
             limit $5
         )
         returning {ENTITLEMENT_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(ctx.now())
    .bind(reason)
    .bind(MAX_ENTITLEMENTS_PER_ORDER)
    .fetch_all(&mut **tx)
    .await?;

    for entitlement in &revoked {
        ctx.audit(
            tx,
            AuditEntry {
                actor: ctx.actor.clone(),
                action: Action::Settle,
                entity: "order_entitlement",
                entity_id: entitlement.id.as_uuid(),
                summary: serde_json::json!({
                    "order": order_id.to_string(),
                    "reason": reason,
                }),
            },
        )
        .await?;

        ctx.emit(
            tx,
            Event {
                name: "entitlement.revoked",
                entity_id: entitlement.id.as_uuid(),
                payload: serde_json::json!({ "order": order_id, "reason": reason }),
            },
        )
        .await?;
    }

    Ok(revoked)
}

/// What one order granted, revoked ones included: an operator asking why a
/// customer cannot download wants to see the revoked row, not an empty list.
pub async fn entitlements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<OrderEntitlement>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: order_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, OrderEntitlement>(&format!(
        "select {ENTITLEMENT_COLUMNS} from order_entitlement
         where scope = $1 and order_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_ENTITLEMENTS_PER_ORDER)
    .fetch_all(&mut **tx)
    .await?)
}

/// Everything one customer may still download, newest first by way of the
/// cursor's ordering.
///
/// Asks about the customer rather than an order: this is a question about a
/// shopper's library, and there is no one order to name.
pub async fn for_customer(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    paging: Paging,
) -> Result<Page<OrderEntitlement>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Customer {
            id: Some(customer_id.as_uuid()),
        },
    )?;

    let rows = sqlx::query_as::<_, OrderEntitlement>(&format!(
        "select {ENTITLEMENT_COLUMNS} from order_entitlement
         where scope = $1 and customer_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(paging.after.map(|cursor| cursor.at))
    .bind(paging.after.map(|cursor| cursor.id))
    .bind(paging.probe())
    .fetch_all(&mut **tx)
    .await?;

    Ok(Page::build(rows, paging, |row| Cursor {
        at: row.created_at,
        id: row.id.as_uuid(),
    }))
}

/// Mints a download link for something already bought.
///
/// The token is handed back once and never stored: what is kept is its hash,
/// so a leaked table is not a pile of working links.
pub async fn issue_token(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    entitlement_id: OrderEntitlementId,
    as_customer: Option<CustomerId>,
) -> Result<IssuedToken> {
    let entitlement = read(tx, ctx, entitlement_id).await?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: entitlement.order_id.as_uuid(),
            customer: entitlement.customer_id.map(CustomerId::as_uuid),
        },
    )?;
    mine(&entitlement, as_customer)?;

    let now = ctx.now();
    if let Some(refusal) = why_not(&entitlement, now) {
        return Err(Error::conflict(refusal));
    }

    // Never outlives the right it is a link to.
    let mut expires_at = now
        + chrono::Duration::try_minutes(TOKEN_MINUTES)
            .ok_or_else(|| Error::bug("a token lifetime that is not a duration"))?;
    if let Some(until) = entitlement.expires_at {
        expires_at = expires_at.min(until);
    }

    let token = fresh_token(tx).await?;
    let id = EntitlementTokenId::new();

    sqlx::query(
        "insert into entitlement_token
             (id, scope, order_entitlement_id, token_hash, expires_at)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(entitlement_id.as_uuid())
    .bind(fingerprint(&token))
    .bind(expires_at)
    .execute(&mut **tx)
    .await?;

    Ok(IssuedToken {
        id,
        token,
        expires_at,
        entitlement,
    })
}

#[derive(FromRow)]
struct TokenRow {
    id: EntitlementTokenId,
    order_entitlement_id: OrderEntitlementId,
    token_hash: String,
    expires_at: chrono::DateTime<chrono::Utc>,
    revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    order_id: OrderId,
    customer_id: Option<CustomerId>,
}

/// Takes one download against a link, and counts it.
///
/// The count moves in one conditional update, the same shape as a gift card's
/// decrement: two tabs clicking download at the same moment must not both get
/// the last one. Zero rows means refused, and *why* is a second read — asked
/// after the fact, so the answer cannot be raced into the condition.
///
/// Refusals are written down as carefully as successes: `entitlement_access`
/// is what a chargeback is answered from, and "they tried eleven times" is
/// part of the answer.
pub async fn redeem(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    token: &str,
    access: Access,
) -> Result<Download> {
    let hashed = fingerprint(token);

    let found = sqlx::query_as::<_, TokenRow>(
        "select t.id, t.order_entitlement_id, t.token_hash, t.expires_at, t.revoked_at,
                e.order_id, e.customer_id
         from entitlement_token t
         join order_entitlement e on e.scope = t.scope and e.id = t.order_entitlement_id
         where t.scope = $1 and t.token_hash = $2",
    )
    .bind(ctx.scope.0)
    .bind(&hashed)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("download link"))?;

    // Constant time, so a wrong token cannot be improved by timing how wrong
    // it was.
    let matches: bool = hashed.as_bytes().ct_eq(found.token_hash.as_bytes()).into();
    if !matches {
        return Err(Error::not_found("download link"));
    }

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: found.order_id.as_uuid(),
            customer: found.customer_id.map(CustomerId::as_uuid),
        },
    )?;
    if access.as_customer.is_some() && access.as_customer != found.customer_id {
        return Err(Error::not_found("download link"));
    }

    let now = ctx.now();
    if found.revoked_at.is_some() || found.expires_at <= now {
        write_access(
            tx,
            ctx,
            found.order_entitlement_id,
            Some(found.id),
            "refused",
            &access,
        )
        .await?;
        return Err(Error::conflict("that download link has expired"));
    }

    let taken = sqlx::query_as::<_, OrderEntitlement>(&format!(
        "update order_entitlement set download_count = download_count + 1
         where scope = $1 and id = $2 and revoked_at is null
           and (expires_at is null or expires_at > $3)
           and (max_downloads is null or download_count < max_downloads)
         returning {ENTITLEMENT_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(found.order_entitlement_id.as_uuid())
    .bind(now)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(entitlement) = taken else {
        write_access(
            tx,
            ctx,
            found.order_entitlement_id,
            Some(found.id),
            "refused",
            &access,
        )
        .await?;
        let entitlement = read(tx, ctx, found.order_entitlement_id).await?;
        return Err(Error::conflict(
            why_not(&entitlement, now).unwrap_or("that download cannot be taken"),
        ));
    };

    write_access(tx, ctx, entitlement.id, Some(found.id), "served", &access).await?;

    if entitlement.remaining() == Some(0) {
        ctx.emit(
            tx,
            Event {
                name: "entitlement.exhausted",
                entity_id: entitlement.id.as_uuid(),
                payload: serde_json::json!({
                    "order": entitlement.order_id,
                    "customer": entitlement.customer_id,
                    "downloads": entitlement.download_count,
                }),
            },
        )
        .await?;
    }

    Ok(Download {
        entitlement_id: entitlement.id,
        content_key: entitlement.content_key.clone(),
        download_count: entitlement.download_count,
        remaining: entitlement.remaining(),
    })
}

// ---------------------------------------------------------------------------
// The small shared parts
// ---------------------------------------------------------------------------

async fn read(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderEntitlementId) -> Result<OrderEntitlement> {
    sqlx::query_as::<_, OrderEntitlement>(&format!(
        "select {ENTITLEMENT_COLUMNS} from order_entitlement where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("entitlement"))
}

/// Somebody else's library is not found rather than refused: which
/// entitlements exist is not a storefront's business either.
fn mine(entitlement: &OrderEntitlement, as_customer: Option<CustomerId>) -> Result<()> {
    match as_customer {
        Some(_) if as_customer != entitlement.customer_id => Err(Error::not_found("entitlement")),
        _ => Ok(()),
    }
}

fn why_not(
    entitlement: &OrderEntitlement,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<&'static str> {
    if entitlement.revoked_at.is_some() {
        return Some("that download was withdrawn");
    }
    if entitlement.expires_at.is_some_and(|at| at <= now) {
        return Some("that download has expired");
    }
    if entitlement.remaining() == Some(0) {
        return Some("every download of that has been taken");
    }
    None
}

async fn write_access(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    entitlement_id: OrderEntitlementId,
    token_id: Option<EntitlementTokenId>,
    outcome: &str,
    access: &Access,
) -> Result<()> {
    sqlx::query(
        "insert into entitlement_access
             (id, scope, order_entitlement_id, entitlement_token_id, at, ip, user_agent, outcome)
         values ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .bind(entitlement_id.as_uuid())
    .bind(token_id.map(EntitlementTokenId::as_uuid))
    .bind(ctx.now())
    .bind(access.ip.as_deref())
    .bind(access.user_agent.as_deref())
    .bind(outcome)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// 256 bits from the database's own generator, so no host has to supply one.
async fn fresh_token(tx: &mut Tx<'_>) -> Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "select replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', '')",
    )
    .fetch_one(&mut **tx)
    .await?)
}

/// A fourth private copy of the same three lines, and deliberately so: the
/// three in `order`, `credit` and `store` are private, and a shared one would
/// be an arrow between domains that own nothing in common but a hash.
fn fingerprint(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}
