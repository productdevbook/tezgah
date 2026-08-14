//! Gift cards and store credit: money the shop already holds.
//!
//! Two instruments, and the difference between them is the whole design.
//!
//! A **gift card** is a product somebody bought. It carries a code, it has a
//! balance, and whoever holds the code spends it. Selling one takes money in
//! that is not revenue: it is a liability until it is redeemed, which is why
//! [`issue`] writes no tax and why `order_line_item.is_giftcard` refuses to
//! carry a tax line.
//!
//! **Store credit** is a named customer's balance. A return can leave the money
//! here instead of sending it back to a card — [`refund_to_credit`] — which is
//! the case that bites every shop and the reason issue #78 was opened.
//!
//! # Neither is a discount
//!
//! A discount changes what the goods cost, and therefore what tax is owed on
//! them. These do not: the basket comes to the same amount, the same tax is
//! charged on it, and an instrument carries part of what is due. So a
//! redemption lands on the [`payment_collection`](crate::payment) as credit
//! rather than on a line as an adjustment, and the card is asked for the rest.
//!
//! # Neither is a payment provider
//!
//! No `payment` row, no session, no provider on the far side of a network. The
//! money is already in the shop's hands; redeeming is a decrement inside the
//! same transaction that reserves the stock, and it compensates by putting the
//! balance back.
//!
//! # The balance and the ledger
//!
//! Every movement is its own row. The `balance` column exists so a redemption
//! is one conditional `update` — never a select and then an update, which is
//! how two shoppers spend the last fifty lira of one card — and the ledger is
//! what that column has to equal. `tests/credit.rs` says so.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    CartCreditId, CartId, CustomerId, GiftCardId, GiftCardTransactionId, OrderId,
    PaymentCollectionId, StoreCreditId, StoreCreditTransactionId,
};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Actor, AuditEntry, Ctx, Event, Permit, Resource, Tx};

/// Most instruments one cart may put against one order. A basket paid with a
/// hundred gift cards is somebody working through a stolen list, not a shopper.
pub const MAX_CART_CREDITS: i64 = 20;

const CARD_COLUMNS: &str = "id, initial_balance, balance, currency_code, issued_order_id, \
                            customer_id, expires_at, disabled_at, created_at, updated_at";

const CARD_LEDGER_COLUMNS: &str = "id, gift_card_id, kind, amount, currency_code, order_id, \
                                   payment_collection_id, reason, created_by, created_at";

const CREDIT_COLUMNS: &str =
    "id, customer_id, currency_code, balance, disabled_at, created_at, updated_at";

const CREDIT_LEDGER_COLUMNS: &str = "id, store_credit_id, kind, amount, currency_code, order_id, \
                                     payment_collection_id, reason, created_by, created_at";

const CART_CREDIT_COLUMNS: &str =
    "id, cart_id, gift_card_id, store_credit_id, amount, currency_code, created_at";

// ---------------------------------------------------------------------------
// What the rows look like
// ---------------------------------------------------------------------------

/// A bearer instrument. The code is not here and cannot be read back: only its
/// hash is stored, so a leaked table is not a pile of spendable cards.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct GiftCard {
    pub id: GiftCardId,
    pub initial_balance: Decimal,
    pub balance: Decimal,
    pub currency_code: String,
    /// The order this card was sold on, when it was sold rather than granted.
    pub issued_order_id: Option<OrderId>,
    pub customer_id: Option<CustomerId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl GiftCard {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn remaining(&self) -> Result<Money> {
        Ok(Money::new(self.balance, self.currency()?))
    }

    pub fn is_spendable(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        self.disabled_at.is_none()
            && self.expires_at.is_none_or(|at| at > now)
            && self.balance > Decimal::ZERO
    }
}

/// A fresh card and the one time its code is readable.
///
/// The code is not stored and cannot be asked for again: whoever issued it has
/// to hand it over now or issue another.
#[derive(Debug, Clone)]
pub struct IssuedGiftCard {
    pub card: GiftCard,
    pub code: String,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct GiftCardTransaction {
    pub id: GiftCardTransactionId,
    pub gift_card_id: GiftCardId,
    /// `issue`, `redeem`, `refund` or `adjust`.
    pub kind: String,
    /// Signed the way the balance moved: a redemption is negative.
    pub amount: Decimal,
    pub currency_code: String,
    pub order_id: Option<OrderId>,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub reason: Option<String>,
    pub created_by: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// One customer's balance in one currency.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoreCredit {
    pub id: StoreCreditId,
    pub customer_id: CustomerId,
    pub currency_code: String,
    pub balance: Decimal,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl StoreCredit {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    pub fn remaining(&self) -> Result<Money> {
        Ok(Money::new(self.balance, self.currency()?))
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct StoreCreditTransaction {
    pub id: StoreCreditTransactionId,
    pub store_credit_id: StoreCreditId,
    pub kind: String,
    pub amount: Decimal,
    pub currency_code: String,
    pub order_id: Option<OrderId>,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub reason: Option<String>,
    pub created_by: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// What the shopper said they would pay with, before any money moves.
///
/// Exactly one of the two ids is set. Claiming the balance happens inside the
/// checkout, the way a promotion is claimed there rather than counted when the
/// provider answers.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct CartCredit {
    pub id: CartCreditId,
    pub cart_id: CartId,
    pub gift_card_id: Option<GiftCardId>,
    pub store_credit_id: Option<StoreCreditId>,
    pub amount: Decimal,
    pub currency_code: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Which unit of which order line printed a card. Two cards on a line of two,
/// and the unique index behind this is what makes a redelivered webhook print
/// nothing the second time.
#[derive(Debug, Clone, Copy)]
pub struct PurchasedLine {
    pub line_item_id: Uuid,
    pub ordinal: i32,
}

/// What a card is being sold or granted as.
#[derive(Debug, Clone)]
pub struct NewGiftCard {
    pub balance: Money,
    /// The order it was sold on. `None` is a card the shop gave away.
    pub issued_order_id: Option<OrderId>,
    /// Who it was issued to, when that is known. A card is a bearer
    /// instrument, so this is a record rather than a restriction.
    pub customer_id: Option<CustomerId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason: Option<String>,
    /// The line that bought it. `None` is a card the operator granted.
    pub line: Option<PurchasedLine>,
}

/// What a movement was for, so the ledger reconciles against the order and the
/// payment collection rather than only against itself.
#[derive(Debug, Clone, Default)]
pub struct Redemption {
    pub order_id: Option<OrderId>,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Gift cards
// ---------------------------------------------------------------------------

/// Mints a card and hands back its code once.
///
/// [`Action::Settle`] rather than [`Action::Write`]: this makes money the shop
/// will have to honour, and a shop that lets somebody edit a product has not
/// thereby let them print cards.
pub async fn issue(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewGiftCard) -> Result<IssuedGiftCard> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: None,
            customer: new.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    if new.balance.amount <= Decimal::ZERO {
        return Err(Error::invalid("a gift card is worth more than nothing"));
    }
    if new.expires_at.is_some_and(|at| at <= ctx.now()) {
        return Err(Error::invalid("a gift card that has already expired"));
    }

    if let Some(order_id) = new.issued_order_id {
        refuse_taxed_giftcard_line(tx, ctx, order_id).await?;
    }

    let code = fresh_code(tx).await?;
    let id = GiftCardId::new();

    let card = sqlx::query_as::<_, GiftCard>(&format!(
        "insert into gift_card
             (id, scope, code_hash, initial_balance, balance, currency_code, issued_order_id,
              customer_id, expires_at, issued_line_item_id, issued_line_ordinal)
         values ($1, $2, $3, $4, $4, $5, $6, $7, $8, $9, $10)
         returning {CARD_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(digest(&code))
    .bind(new.balance.amount)
    .bind(new.balance.currency.as_str())
    .bind(new.issued_order_id.map(OrderId::as_uuid))
    .bind(new.customer_id.map(CustomerId::as_uuid))
    .bind(new.expires_at)
    .bind(new.line.map(|line| line.line_item_id))
    .bind(new.line.map(|line| line.ordinal))
    .fetch_one(&mut **tx)
    .await?;

    write_card_row(
        tx,
        ctx,
        id,
        "issue",
        new.balance,
        &Redemption {
            order_id: new.issued_order_id,
            payment_collection_id: None,
            reason: new.reason,
        },
    )
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "gift_card",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "balance": new.balance.amount.to_string(),
                "currency": new.balance.currency.as_str(),
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "gift_card.issued",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "order": new.issued_order_id,
                "currency": new.balance.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(IssuedGiftCard { card, code })
}

/// Most cards one order may print. A basket of a thousand cards is somebody
/// laundering a stolen card, not a shopper buying presents.
pub const MAX_PURCHASED_CARDS: i64 = 200;

#[derive(FromRow)]
struct PurchasedRow {
    id: Uuid,
    unit_price: Decimal,
    currency_code: String,
    quantity: i32,
    customer_id: Option<CustomerId>,
}

/// Prints the cards an order bought, once the shop actually has the money.
///
/// Authorising is not paying: a checkout only puts a hold on a card, and
/// handing over a spendable balance against a hold is giving the goods away
/// before the till rings. So this is called where money is taken — after a
/// capture — and never from the checkout.
///
/// It is safe to call again, which is the point: a provider redelivers its
/// webhook, and the second delivery finds every unit already printed and
/// prints nothing. `gift_card_issued_line_key` is what makes that true against
/// two deliveries arriving at once rather than one after the other.
///
/// The code goes out in the `gift_card.purchased` event and is never returned
/// to the caller. tezgah does not know the buyer's e-mail address and will not
/// learn it; only the hash is kept, so this event is the single moment the code
/// exists outside the buyer's hands, and the host's outbox is where the shop
/// decides what to do with it.
pub async fn issue_purchased(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<Vec<GiftCardId>> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: None,
            customer: None,
        },
    )?;

    let lines = sqlx::query_as::<_, PurchasedRow>(
        r#"select l.id, l.unit_price, l.currency_code, i.quantity, o.customer_id
           from order_line_item l
           join "order" o on o.scope = l.scope and o.id = l.order_id
           join order_item i
             on i.scope = l.scope and i.order_line_item_id = l.id and i.version = o.version
           where l.scope = $1 and l.order_id = $2 and l.is_giftcard and i.quantity > 0
           order by l.created_at, l.id
           limit $3"#,
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .bind(MAX_PURCHASED_CARDS)
    .fetch_all(&mut **tx)
    .await?;

    if lines.is_empty() {
        return Ok(Vec::new());
    }

    let owed: Decimal = lines
        .iter()
        .map(|line| line.unit_price * Decimal::from(line.quantity))
        .sum();

    // Only money the shop is holding: a `payment` row is an authorisation, and
    // a hold is not a payment. A partial capture below what the cards are worth
    // has not bought them, and the next capture finds them still unprinted.
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

    let line_ids: Vec<Uuid> = lines.iter().map(|line| line.id).collect();
    let printed: Vec<(Uuid, i32)> = sqlx::query_as(
        "select issued_line_item_id, issued_line_ordinal from gift_card
         where scope = $1 and issued_line_item_id = any($2)",
    )
    .bind(ctx.scope.0)
    .bind(&line_ids)
    .fetch_all(&mut **tx)
    .await?;

    let mut issued = Vec::new();
    for line in &lines {
        let currency = Currency::parse(&line.currency_code)?;
        for ordinal in 1..=line.quantity {
            if printed
                .iter()
                .any(|(item, at)| *item == line.id && *at == ordinal)
            {
                continue;
            }

            let card = issue(
                tx,
                ctx,
                NewGiftCard {
                    balance: Money::new(line.unit_price, currency),
                    issued_order_id: Some(order_id),
                    customer_id: line.customer_id,
                    expires_at: None,
                    reason: Some("purchased".to_string()),
                    line: Some(PurchasedLine {
                        line_item_id: line.id,
                        ordinal,
                    }),
                },
            )
            .await?;

            ctx.emit(
                tx,
                Event {
                    name: "gift_card.purchased",
                    entity_id: card.card.id.as_uuid(),
                    payload: serde_json::json!({
                        "order": order_id,
                        "line": line.id,
                        "ordinal": ordinal,
                        "balance": line.unit_price.to_string(),
                        "currency": currency.as_str(),
                        "code": card.code,
                    }),
                },
            )
            .await?;

            issued.push(card.card.id);
        }
    }

    Ok(issued)
}

pub async fn gift_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: GiftCardId) -> Result<GiftCard> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    read_card(tx, ctx, id).await
}

/// The card a code names, and nothing when it names none.
///
/// The lookup is by hash, and the hash that comes back is compared in constant
/// time: a comparison that stops at the first wrong byte tells whoever is
/// guessing how much of the code they have right.
pub async fn gift_card_by_code(tx: &mut Tx<'_>, ctx: &Ctx<'_>, code: &str) -> Result<GiftCard> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: None,
            customer: None,
        },
    )?;

    #[derive(FromRow)]
    struct Found {
        code_hash: String,
    }

    let hashed = digest(code);

    let found = sqlx::query_as::<_, Found>(
        "select code_hash from gift_card where scope = $1 and code_hash = $2",
    )
    .bind(ctx.scope.0)
    .bind(&hashed)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("gift card"))?;

    let matches: bool = hashed.as_bytes().ct_eq(found.code_hash.as_bytes()).into();
    if !matches {
        return Err(Error::not_found("gift card"));
    }

    sqlx::query_as::<_, GiftCard>(&format!(
        "select {CARD_COLUMNS} from gift_card where scope = $1 and code_hash = $2"
    ))
    .bind(ctx.scope.0)
    .bind(&hashed)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("gift card"))
}

pub async fn gift_cards(tx: &mut Tx<'_>, ctx: &Ctx<'_>, paging: Paging) -> Result<Page<GiftCard>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: None,
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, GiftCard>(&format!(
        "select {CARD_COLUMNS} from gift_card
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

    Ok(Page::build(rows, paging, |card| Cursor {
        at: card.created_at,
        id: card.id.as_uuid(),
    }))
}

/// Stops a card being spent without destroying what it did. A lost card is
/// disabled and another is issued; nothing is deleted, because the ledger is
/// the only account of where the money went.
pub async fn disable_gift_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: GiftCardId) -> Result<GiftCard> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    let card = sqlx::query_as::<_, GiftCard>(&format!(
        "update gift_card set disabled_at = coalesce(disabled_at, $3)
         where scope = $1 and id = $2
         returning {CARD_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("gift card"))?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "gift_card",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "disabled": true }),
        },
    )
    .await?;

    Ok(card)
}

pub async fn gift_card_ledger(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    paging: Paging,
) -> Result<Page<GiftCardTransaction>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, GiftCardTransaction>(&format!(
        "select {CARD_LEDGER_COLUMNS} from gift_card_transaction
         where scope = $1
           and gift_card_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
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

/// Takes an amount off a card, or refuses.
///
/// One statement. The balance is never read and then written: two shoppers
/// spending the last fifty lira of one card is the race this exists to lose
/// safely, and the loser is told the card has not got it rather than the card
/// going negative.
pub async fn redeem_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    amount: Money,
    what: &Redemption,
) -> Result<GiftCardTransaction> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a redemption is for more than nothing"));
    }

    let taken = sqlx::query_as::<_, GiftCardTransaction>(&format!(
        "with taken as (
             update gift_card
                set balance = balance - $4
              where scope = $1
                and id = $2
                and disabled_at is null
                and (expires_at is null or expires_at > $3)
                and currency_code = $5
                and balance >= $4
             returning id
         )
         insert into gift_card_transaction
             (id, scope, gift_card_id, kind, amount, currency_code, order_id,
              payment_collection_id, reason, created_by)
         select $6, $1, taken.id, 'redeem', -$4, $5, $7, $8, $9, $10 from taken
         returning {CARD_LEDGER_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(ctx.now())
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(GiftCardTransactionId::new().as_uuid())
    .bind(what.order_id.map(OrderId::as_uuid))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .bind(what.reason.as_deref())
    .bind(who(&ctx.actor))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        Error::conflict("that gift card has expired, been disabled, or has not got that much on it")
    })?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "gift_card",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "redeemed": amount.amount.to_string(),
                "order": what.order_id,
            }),
        },
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "gift_card.redeemed",
            entity_id: id.as_uuid(),
            payload: serde_json::json!({
                "amount": amount.amount.to_string(),
                "currency": amount.currency.as_str(),
                "order": what.order_id,
            }),
        },
    )
    .await?;

    settle(tx, ctx, what).await?;

    Ok(taken)
}

/// Puts a redemption back, for a checkout that did not become an order.
///
/// Written so running it twice is running it once: the ledger row it would
/// write is the condition on the balance moving, so a compensation replayed by
/// the workflow runner cannot credit the card twice.
pub async fn restore_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    amount: Money,
    what: &Redemption,
) -> Result<()> {
    let order_id = what
        .order_id
        .ok_or_else(|| Error::invalid("a restoration says which order it undoes"))?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a restoration is for more than nothing"));
    }

    sqlx::query(
        "with put as (
             insert into gift_card_transaction
                 (id, scope, gift_card_id, kind, amount, currency_code, order_id,
                  payment_collection_id, created_by)
             select $3, $1, $2, 'refund', $4, $5, $6, $8, $7
             where not exists (
                 select 1 from gift_card_transaction
                 where scope = $1 and gift_card_id = $2 and kind = 'refund' and order_id = $6
             )
             returning amount
         )
         update gift_card
            set balance = balance + coalesce((select amount from put), 0)
          where scope = $1 and id = $2 and exists (select 1 from put)",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(GiftCardTransactionId::new().as_uuid())
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(order_id.as_uuid())
    .bind(who(&ctx.actor))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .execute(&mut **tx)
    .await?;

    settle(tx, ctx, what).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Store credit
// ---------------------------------------------------------------------------

/// The customer's balance in one currency, and nothing when they have none.
pub async fn store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    currency: Currency,
) -> Result<StoreCredit> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: None,
            customer: Some(customer_id.as_uuid()),
        },
    )?;

    read_credit(tx, ctx, customer_id, currency).await
}

/// The same account by its own id, for a caller holding one rather than a
/// customer and a currency.
pub async fn store_credit_by_id(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
) -> Result<StoreCredit> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    sqlx::query_as::<_, StoreCredit>(&format!(
        "select {CREDIT_COLUMNS} from store_credit where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("store credit"))
}

/// Puts money on a customer's balance, opening the account if this is the
/// first time.
pub async fn grant_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    amount: Money,
    reason: Option<String>,
) -> Result<StoreCredit> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: None,
            customer: Some(customer_id.as_uuid()),
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a grant is for more than nothing"));
    }

    let (account, _) = add_credit(
        tx,
        ctx,
        customer_id,
        amount,
        "issue",
        &Redemption {
            reason,
            ..Redemption::default()
        },
    )
    .await?;

    Ok(account)
}

/// Sends a refund to the customer's balance instead of back to the card.
///
/// The order ledger still records the money going out — it is a refund and the
/// order says so — and the balance records it arriving. What did not happen is
/// a provider call, which is the point: the shop keeps the cash and the fee.
pub async fn refund_to_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
    amount: Money,
    reason: Option<String>,
) -> Result<StoreCredit> {
    let owner: Option<(Option<Uuid>, String)> = sqlx::query_as(
        r#"select customer_id, currency_code from "order" where scope = $1 and id = $2"#,
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    let (customer_id, currency_code) = owner.ok_or_else(|| Error::not_found("order"))?;
    let customer_id = customer_id
        .map(CustomerId::from_uuid)
        .ok_or_else(|| Error::invalid("a guest order has nobody to hold the credit"))?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: None,
            customer: Some(customer_id.as_uuid()),
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a refund is for more than nothing"));
    }
    if amount.currency.as_str() != currency_code {
        return Err(Error::invalid(format!(
            "this order is in {currency_code}, not {}",
            amount.currency
        )));
    }

    let (account, movement) = add_credit(
        tx,
        ctx,
        customer_id,
        amount,
        "refund",
        &Redemption {
            order_id: Some(order_id),
            payment_collection_id: None,
            reason,
        },
    )
    .await?;

    // The ledger row rather than the account: a customer refunded twice out of
    // one order is two movements, and naming the account would make the second
    // meet the unique index and vanish.
    crate::order::record_transaction(
        tx,
        ctx,
        order_id,
        Money::new(-amount.amount, amount.currency),
        "refund",
        movement.as_uuid(),
    )
    .await?;

    ctx.emit(
        tx,
        Event {
            name: "store_credit.refunded",
            entity_id: account.id.as_uuid(),
            payload: serde_json::json!({
                "order": order_id,
                "amount": amount.amount.to_string(),
                "currency": amount.currency.as_str(),
            }),
        },
    )
    .await?;

    Ok(account)
}

pub async fn store_credit_ledger(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
    paging: Paging,
) -> Result<Page<StoreCreditTransaction>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    let rows = sqlx::query_as::<_, StoreCreditTransaction>(&format!(
        "select {CREDIT_LEDGER_COLUMNS} from store_credit_transaction
         where scope = $1
           and store_credit_id = $2
           and ($3::timestamptz is null or (created_at, id) > ($3, $4))
         order by created_at, id
         limit $5"
    ))
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

/// The same conditional decrement as [`redeem_gift_card`], on a named
/// customer's balance.
pub async fn redeem_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
    amount: Money,
    what: &Redemption,
) -> Result<StoreCreditTransaction> {
    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a redemption is for more than nothing"));
    }

    let taken = sqlx::query_as::<_, StoreCreditTransaction>(&format!(
        "with taken as (
             update store_credit
                set balance = balance - $3
              where scope = $1
                and id = $2
                and disabled_at is null
                and currency_code = $4
                and balance >= $3
             returning id
         )
         insert into store_credit_transaction
             (id, scope, store_credit_id, kind, amount, currency_code, order_id,
              payment_collection_id, reason, created_by)
         select $5, $1, taken.id, 'redeem', -$3, $4, $6, $7, $8, $9 from taken
         returning {CREDIT_LEDGER_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(StoreCreditTransactionId::new().as_uuid())
    .bind(what.order_id.map(OrderId::as_uuid))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .bind(what.reason.as_deref())
    .bind(who(&ctx.actor))
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| {
        Error::conflict("that balance has been disabled, or has not got that much on it")
    })?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "store_credit",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({
                "redeemed": amount.amount.to_string(),
                "order": what.order_id,
            }),
        },
    )
    .await?;

    settle(tx, ctx, what).await?;

    Ok(taken)
}

/// The inverse of [`redeem_store_credit`], written the same way so a
/// compensation that runs twice puts the money back once.
pub async fn restore_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
    amount: Money,
    what: &Redemption,
) -> Result<()> {
    let order_id = what
        .order_id
        .ok_or_else(|| Error::invalid("a restoration says which order it undoes"))?;

    let _: Permit = ctx.permit(
        Action::Settle,
        Resource::Credit {
            id: Some(id.as_uuid()),
            customer: None,
        },
    )?;

    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("a restoration is for more than nothing"));
    }

    sqlx::query(
        "with put as (
             insert into store_credit_transaction
                 (id, scope, store_credit_id, kind, amount, currency_code, order_id,
                  payment_collection_id, created_by)
             select $3, $1, $2, 'refund', $4, $5, $6, $8, $7
             where not exists (
                 select 1 from store_credit_transaction
                 where scope = $1 and store_credit_id = $2 and kind = 'refund' and order_id = $6
             )
             returning amount
         )
         update store_credit
            set balance = balance + coalesce((select amount from put), 0)
          where scope = $1 and id = $2 and exists (select 1 from put)",
    )
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(StoreCreditTransactionId::new().as_uuid())
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(order_id.as_uuid())
    .bind(who(&ctx.actor))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .execute(&mut **tx)
    .await?;

    settle(tx, ctx, what).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Saying so at the cart, claiming it at the checkout
// ---------------------------------------------------------------------------

/// Says this cart intends to spend a card, without spending it.
///
/// Nothing moves here on purpose: a balance claimed while somebody browses is
/// a balance held for as long as they browse. The checkout claims it inside the
/// transaction that reserves the stock, and gives it back when that unwinds.
pub async fn apply_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    code: &str,
    amount: Money,
) -> Result<CartCredit> {
    let card = gift_card_by_code(tx, ctx, code).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    if !card.is_spendable(ctx.now()) {
        return Err(Error::conflict(
            "that gift card has expired, been disabled, or has nothing left on it",
        ));
    }
    if card.currency_code != amount.currency.as_str() {
        return Err(Error::invalid(
            "that gift card is not in this cart's currency",
        ));
    }

    put_cart_credit(tx, ctx, cart_id, Some(card.id), None, amount).await
}

/// The same for the signed-in customer's own balance.
pub async fn apply_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    customer_id: CustomerId,
    amount: Money,
) -> Result<CartCredit> {
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: Some(customer_id.as_uuid()),
        },
    )?;

    let account = read_credit(tx, ctx, customer_id, amount.currency).await?;
    if account.disabled_at.is_some() {
        return Err(Error::conflict("that balance has been disabled"));
    }

    put_cart_credit(tx, ctx, cart_id, None, Some(account.id), amount).await
}

/// What this cart means to pay with, oldest first and capped at
/// [`MAX_CART_CREDITS`].
pub async fn cart_credits(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
) -> Result<Vec<CartCredit>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Cart {
            id: cart_id.as_uuid(),
            customer: None,
        },
    )?;

    Ok(sqlx::query_as::<_, CartCredit>(&format!(
        "select {CART_CREDIT_COLUMNS} from cart_credit
         where scope = $1 and cart_id = $2
         order by created_at, id
         limit $3"
    ))
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(MAX_CART_CREDITS)
    .fetch_all(&mut **tx)
    .await?)
}

pub async fn remove_cart_credit(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartCreditId) -> Result<()> {
    let cart_id: Option<Uuid> =
        sqlx::query_scalar("select cart_id from cart_credit where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(id.as_uuid())
            .fetch_optional(&mut **tx)
            .await?;

    let cart_id = cart_id.ok_or_else(|| Error::not_found("cart credit"))?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Cart {
            id: cart_id,
            customer: None,
        },
    )?;

    sqlx::query("delete from cart_credit where scope = $1 and id = $2")
        .bind(ctx.scope.0)
        .bind(id.as_uuid())
        .execute(&mut **tx)
        .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// The small shared parts
// ---------------------------------------------------------------------------

async fn put_cart_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    gift_card_id: Option<GiftCardId>,
    store_credit_id: Option<StoreCreditId>,
    amount: Money,
) -> Result<CartCredit> {
    if amount.amount <= Decimal::ZERO {
        return Err(Error::invalid("an instrument carries more than nothing"));
    }

    // The unique index is partial and only covers the id that is set, so the
    // conflict target has to name the one this row is using.
    let conflict = if gift_card_id.is_some() {
        "(scope, cart_id, gift_card_id) where gift_card_id is not null"
    } else {
        "(scope, cart_id, store_credit_id) where store_credit_id is not null"
    };

    Ok(sqlx::query_as::<_, CartCredit>(&format!(
        "insert into cart_credit
             (id, scope, cart_id, gift_card_id, store_credit_id, amount, currency_code)
         values ($1, $2, $3, $4, $5, $6, $7)
         on conflict {conflict} do update set amount = excluded.amount
         returning {CART_CREDIT_COLUMNS}"
    ))
    .bind(CartCreditId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .bind(gift_card_id.map(GiftCardId::as_uuid))
    .bind(store_credit_id.map(StoreCreditId::as_uuid))
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .fetch_one(&mut **tx)
    .await?)
}

/// A movement against a payment collection changes what is left for a card, so
/// the collection is brought back into line by the act that changed it rather
/// than by whoever remembers to ask.
async fn settle(tx: &mut Tx<'_>, ctx: &Ctx<'_>, what: &Redemption) -> Result<()> {
    if let Some(collection_id) = what.payment_collection_id {
        crate::payment::recompute(tx, ctx, collection_id).await?;
    }

    Ok(())
}

async fn read_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: GiftCardId) -> Result<GiftCard> {
    sqlx::query_as::<_, GiftCard>(&format!(
        "select {CARD_COLUMNS} from gift_card where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("gift card"))
}

async fn read_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    currency: Currency,
) -> Result<StoreCredit> {
    sqlx::query_as::<_, StoreCredit>(&format!(
        "select {CREDIT_COLUMNS} from store_credit
         where scope = $1 and customer_id = $2 and currency_code = $3"
    ))
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(currency.as_str())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("store credit"))
}

/// Opens the account if it is the customer's first credit, adds to it, and
/// writes the ledger row that has to match.
async fn add_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    amount: Money,
    kind: &str,
    what: &Redemption,
) -> Result<(StoreCredit, StoreCreditTransactionId)> {
    let account = sqlx::query_as::<_, StoreCredit>(&format!(
        "insert into store_credit (id, scope, customer_id, currency_code, balance)
         values ($1, $2, $3, $4, $5)
         on conflict (scope, customer_id, currency_code) do update
             set balance = store_credit.balance + excluded.balance
         returning {CREDIT_COLUMNS}"
    ))
    .bind(StoreCreditId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(customer_id.as_uuid())
    .bind(amount.currency.as_str())
    .bind(amount.amount)
    .fetch_one(&mut **tx)
    .await?;

    let movement = StoreCreditTransactionId::new();
    sqlx::query(
        "insert into store_credit_transaction
             (id, scope, store_credit_id, kind, amount, currency_code, order_id,
              payment_collection_id, reason, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(movement.as_uuid())
    .bind(ctx.scope.0)
    .bind(account.id.as_uuid())
    .bind(kind)
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(what.order_id.map(OrderId::as_uuid))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .bind(what.reason.as_deref())
    .bind(who(&ctx.actor))
    .execute(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Settle,
            entity: "store_credit",
            entity_id: account.id.as_uuid(),
            summary: serde_json::json!({
                "kind": kind,
                "amount": amount.amount.to_string(),
            }),
        },
    )
    .await?;

    Ok((account, movement))
}

async fn write_card_row(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    kind: &str,
    amount: Money,
    what: &Redemption,
) -> Result<()> {
    sqlx::query(
        "insert into gift_card_transaction
             (id, scope, gift_card_id, kind, amount, currency_code, order_id,
              payment_collection_id, reason, created_by)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(GiftCardTransactionId::new().as_uuid())
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(kind)
    .bind(amount.amount)
    .bind(amount.currency.as_str())
    .bind(what.order_id.map(OrderId::as_uuid))
    .bind(what.payment_collection_id.map(PaymentCollectionId::as_uuid))
    .bind(what.reason.as_deref())
    .bind(who(&ctx.actor))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

/// Selling a gift card is not selling goods: the money is a liability until it
/// is spent, and the tax is charged on whatever it eventually buys. So a line
/// marked `is_giftcard` carries no tax line, and a card issued from one that
/// somehow does is refused here rather than quietly sold with tax on it.
async fn refuse_taxed_giftcard_line(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    order_id: OrderId,
) -> Result<()> {
    let taxed: Option<Uuid> = sqlx::query_scalar(
        "select l.id from order_line_item l
         join order_line_item_tax_line t
           on t.scope = l.scope and t.order_line_item_id = l.id
         where l.scope = $1 and l.order_id = $2 and l.is_giftcard
         limit 1",
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?;

    if taxed.is_some() {
        return Err(Error::invalid(
            "a gift card line carries tax; the tax belongs on what the card buys",
        ));
    }

    Ok(())
}

/// 256 bits from the database's own generator, so no host has to supply one.
async fn fresh_code(tx: &mut Tx<'_>) -> Result<String> {
    Ok(sqlx::query_scalar::<_, String>(
        "select upper(replace(gen_random_uuid()::text || gen_random_uuid()::text, '-', ''))",
    )
    .fetch_one(&mut **tx)
    .await?)
}

fn digest(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.trim().to_uppercase().as_bytes());
    hex::encode(hasher.finalize())
}

fn who(actor: &Actor) -> Option<String> {
    match actor {
        Actor::Staff { id } | Actor::Customer { id } => Some(id.to_string()),
        Actor::Guest { cart } => Some(cart.to_string()),
        Actor::System => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_hashes_the_same_however_it_was_typed() {
        assert_eq!(digest(" abc123 "), digest("ABC123"));
    }

    #[test]
    fn two_codes_do_not_hash_alike() {
        assert_ne!(digest("abc123"), digest("abc124"));
    }
}
