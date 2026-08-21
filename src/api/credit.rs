//! Gift cards and store credit, on both surfaces.
//!
//! Money the shop already holds rather than money a provider holds, which is
//! why issuing a card, disabling one and moving a balance ask
//! [`Action::Settle`] and not [`Action::Write`]: a shop that lets somebody edit
//! a product has not thereby let them print cards.
//!
//! # The code leaves once
//!
//! Only the hash of a card's code is stored, and only [`issue_gift_card`] ever
//! hands the code back — the one moment it exists outside the shopper's hand.
//! Nothing else here carries it: not the list, not the fetch, not the lookup a
//! back office does with a code somebody read out over the telephone. A route
//! that echoed a code would turn a log into a pile of spendable cards.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::credit;
use crate::error::Result;
use crate::id::{CartCreditId, CartId, CustomerId, GiftCardId, OrderId, StoreCreditId};
use crate::money::{Currency, Money};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};

use super::{Method, Route, Surface, own_cart, signed_in};

fn paging(after: Option<&str>, limit: Option<u32>) -> Result<Paging> {
    let limit = limit.unwrap_or(crate::page::DEFAULT_LIMIT);
    match after {
        Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
        None => Ok(Paging::first(limit)),
    }
}

fn map<T, U>(page: Page<T>, into: impl Fn(T) -> U) -> Page<U> {
    page.map(into)
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct List {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl List {
    fn paging(&self) -> Result<Paging> {
        paging(self.after.as_deref(), self.limit)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AmountIn {
    pub amount: Decimal,
    pub currency_code: String,
}

impl AmountIn {
    fn money(&self) -> Result<Money> {
        Ok(Money::new(
            self.amount,
            Currency::parse(&self.currency_code)?,
        ))
    }
}

// ---------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------

/// A card as it leaves the building. No code and no hash: what is left is what
/// is spendable, and who it belongs to when anybody does.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GiftCardView {
    pub id: GiftCardId,
    pub initial_balance: Decimal,
    pub balance: Decimal,
    pub currency_code: String,
    pub issued_order_id: Option<OrderId>,
    pub customer_id: Option<CustomerId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<credit::GiftCard> for GiftCardView {
    fn from(row: credit::GiftCard) -> Self {
        GiftCardView {
            id: row.id,
            initial_balance: row.initial_balance,
            balance: row.balance,
            currency_code: row.currency_code,
            issued_order_id: row.issued_order_id,
            customer_id: row.customer_id,
            expires_at: row.expires_at,
            disabled_at: row.disabled_at,
            created_at: row.created_at,
        }
    }
}

/// The card and the only answer that ever carries its code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssuedGiftCardView {
    pub card: GiftCardView,
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreditMovementView {
    pub id: uuid::Uuid,
    pub kind: String,
    pub amount: Decimal,
    pub currency_code: String,
    pub order_id: Option<OrderId>,
    pub reason: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<credit::GiftCardTransaction> for CreditMovementView {
    fn from(row: credit::GiftCardTransaction) -> Self {
        CreditMovementView {
            id: row.id.as_uuid(),
            kind: row.kind,
            amount: row.amount,
            currency_code: row.currency_code,
            order_id: row.order_id,
            reason: row.reason,
            created_at: row.created_at,
        }
    }
}

impl From<credit::StoreCreditTransaction> for CreditMovementView {
    fn from(row: credit::StoreCreditTransaction) -> Self {
        CreditMovementView {
            id: row.id.as_uuid(),
            kind: row.kind,
            amount: row.amount,
            currency_code: row.currency_code,
            order_id: row.order_id,
            reason: row.reason,
            created_at: row.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct StoreCreditView {
    pub id: StoreCreditId,
    pub customer_id: CustomerId,
    pub currency_code: String,
    pub balance: Decimal,
    pub disabled_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<credit::StoreCredit> for StoreCreditView {
    fn from(row: credit::StoreCredit) -> Self {
        StoreCreditView {
            id: row.id,
            customer_id: row.customer_id,
            currency_code: row.currency_code,
            balance: row.balance,
            disabled_at: row.disabled_at,
        }
    }
}

/// What a cart says it will pay with. Which card is named by id rather than by
/// code, for the same reason the code is not in any other answer.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CartCreditView {
    pub id: CartCreditId,
    pub cart_id: CartId,
    pub gift_card_id: Option<GiftCardId>,
    pub store_credit_id: Option<StoreCreditId>,
    pub amount: Decimal,
    pub currency_code: String,
}

impl From<credit::CartCredit> for CartCreditView {
    fn from(row: credit::CartCredit) -> Self {
        CartCreditView {
            id: row.id,
            cart_id: row.cart_id,
            gift_card_id: row.gift_card_id,
            store_credit_id: row.store_credit_id,
            amount: row.amount,
            currency_code: row.currency_code,
        }
    }
}

// ---------------------------------------------------------------------------
// Gift cards, in the back office
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueGiftCard {
    pub balance: AmountIn,
    pub customer_id: Option<CustomerId>,
    pub issued_order_id: Option<OrderId>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub reason: Option<String>,
}

pub async fn issue_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: IssueGiftCard,
) -> Result<IssuedGiftCardView> {
    let issued = credit::issue(
        tx,
        ctx,
        credit::NewGiftCard {
            balance: body.balance.money()?,
            issued_order_id: body.issued_order_id,
            customer_id: body.customer_id,
            expires_at: body.expires_at,
            reason: body.reason,
            line: None,
        },
    )
    .await?;

    Ok(IssuedGiftCardView {
        card: GiftCardView::from(issued.card),
        code: issued.code,
    })
}

/// A gift card's own query type. No `q`: the code is stored hashed, so there
/// is nothing on the row a substring could match.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListGiftCards {
    pub after: Option<String>,
    pub limit: Option<u32>,
    /// Cards issued to one customer.
    pub customer_id: Option<Uuid>,
    /// A three-letter code.
    pub currency_code: Option<String>,
    /// True for cards stopped by hand.
    pub disabled: Option<bool>,
    /// True for cards with nothing left on them.
    pub spent: Option<bool>,
    /// Which end first. Left out, this surface answers newest-first.
    pub order: Option<crate::page::Order>,
    /// Asks how many cards match, as well as this page of them.
    pub count: Option<bool>,
}

impl ListGiftCards {
    fn listing(&self) -> Result<Paging> {
        let paging = List {
            after: self.after.clone(),
            limit: self.limit,
        }
        .paging()?;

        Ok(if self.count.unwrap_or(false) {
            paging.counting()
        } else {
            paging
        })
    }
}

pub async fn list_gift_cards(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListGiftCards,
) -> Result<Page<GiftCardView>> {
    let filter = credit::GiftCardFilter {
        customer: query.customer_id.map(CustomerId::from_uuid),
        currency: match query.currency_code.as_deref() {
            Some(code) => Some(Currency::parse(code)?),
            None => None,
        },
        disabled: query.disabled,
        spent: query.spent,
        order: query.order.unwrap_or(crate::page::Order::Newest),
    };

    Ok(map(
        credit::gift_cards(tx, ctx, filter, query.listing()?).await?,
        GiftCardView::from,
    ))
}

pub async fn get_gift_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: GiftCardId) -> Result<GiftCardView> {
    Ok(GiftCardView::from(credit::gift_card(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GiftCardCode {
    pub code: String,
}

/// The card a code names. A POST because the code belongs in a body: a path is
/// written down by every proxy between here and whoever asked.
pub async fn find_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    body: GiftCardCode,
) -> Result<GiftCardView> {
    Ok(GiftCardView::from(
        credit::gift_card_by_code(tx, ctx, &body.code).await?,
    ))
}

pub async fn disable_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
) -> Result<GiftCardView> {
    Ok(GiftCardView::from(
        credit::disable_gift_card(tx, ctx, id).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Adjustment {
    pub amount: AmountIn,
    pub reason: String,
}

/// A hand correction to a card's balance, in either direction: a chargeback
/// reversed, a wrong amount put right. Distinct from [`issue_gift_card`] and
/// from a redemption's refund, both of which already carry their own record —
/// this is the one for a movement that has no other route to go through.
pub async fn adjust_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    body: Adjustment,
) -> Result<GiftCardView> {
    credit::adjust_gift_card(tx, ctx, id, body.amount.money()?, &body.reason).await?;
    Ok(GiftCardView::from(credit::gift_card(tx, ctx, id).await?))
}

pub async fn gift_card_movements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: GiftCardId,
    query: List,
) -> Result<Page<CreditMovementView>> {
    Ok(map(
        credit::gift_card_ledger(tx, ctx, id, query.paging()?).await?,
        CreditMovementView::from,
    ))
}

// ---------------------------------------------------------------------------
// Store credit, in the back office
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BalanceQuery {
    pub currency_code: String,
}

pub async fn get_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    query: BalanceQuery,
) -> Result<StoreCreditView> {
    let currency = Currency::parse(&query.currency_code)?;
    Ok(StoreCreditView::from(
        credit::store_credit(tx, ctx, customer_id, currency).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdjustStoreCredit {
    pub amount: AmountIn,
    pub reason: Option<String>,
}

/// Puts money on a customer's balance. Taking it off again is a redemption
/// against an order or a refund, both of which have their own record; there is
/// no route here that lowers a balance for no reason anybody can point at.
pub async fn adjust_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    customer_id: CustomerId,
    body: AdjustStoreCredit,
) -> Result<StoreCreditView> {
    Ok(StoreCreditView::from(
        credit::grant_store_credit(tx, ctx, customer_id, body.amount.money()?, body.reason).await?,
    ))
}

pub async fn store_credit_movements(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
    query: List,
) -> Result<Page<CreditMovementView>> {
    Ok(map(
        credit::store_credit_ledger(tx, ctx, id, query.paging()?).await?,
        CreditMovementView::from,
    ))
}

/// The store-credit half of [`adjust_gift_card`]. `id` names the balance
/// rather than the customer: a customer can carry one per currency, and only
/// one of them is being corrected.
pub async fn adjust_store_credit_balance(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: StoreCreditId,
    body: Adjustment,
) -> Result<StoreCreditView> {
    credit::adjust_store_credit(tx, ctx, id, body.amount.money()?, &body.reason).await?;
    Ok(StoreCreditView::from(
        credit::store_credit_by_id(tx, ctx, id).await?,
    ))
}

// ---------------------------------------------------------------------------
// What a shopper may do with either
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyGiftCard {
    pub code: String,
    pub amount: AmountIn,
}

pub async fn apply_gift_card(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    body: ApplyGiftCard,
) -> Result<CartCreditView> {
    own_cart(tx, ctx, cart_id, Action::Write).await?;

    Ok(CartCreditView::from(
        credit::apply_gift_card(tx, ctx, cart_id, &body.code, body.amount.money()?).await?,
    ))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyStoreCredit {
    pub amount: AmountIn,
}

/// The signed-in shopper's own balance and nobody else's: the customer is read
/// off the actor rather than taken from the body.
pub async fn apply_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    body: ApplyStoreCredit,
) -> Result<CartCreditView> {
    let me = signed_in(ctx)?;
    own_cart(tx, ctx, cart_id, Action::Write).await?;

    Ok(CartCreditView::from(
        credit::apply_store_credit(tx, ctx, cart_id, me, body.amount.money()?).await?,
    ))
}

pub async fn list_cart_credits(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
) -> Result<Vec<CartCreditView>> {
    own_cart(tx, ctx, cart_id, Action::View).await?;

    Ok(credit::cart_credits(tx, ctx, cart_id)
        .await?
        .into_iter()
        .map(CartCreditView::from)
        .collect())
}

pub async fn remove_cart_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    id: CartCreditId,
) -> Result<()> {
    own_cart(tx, ctx, cart_id, Action::Write).await?;

    credit::remove_cart_credit(tx, ctx, id).await
}

pub async fn my_store_credit(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: BalanceQuery,
) -> Result<StoreCreditView> {
    let me = signed_in(ctx)?;
    let currency = Currency::parse(&query.currency_code)?;

    Ok(StoreCreditView::from(
        credit::store_credit(tx, ctx, me, currency).await?,
    ))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/gift-cards",
        action: Action::Settle,
        domain: "credit",
        query: None,
        summary: "Issue a gift card and read its code once",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/gift-cards",
        action: Action::View,
        domain: "credit",
        query: Some(super::query_schema::<ListGiftCards>),
        summary: "List gift cards",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/gift-cards/lookup",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "Find the gift card a code names",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/gift-cards/{id}",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "Fetch one gift card",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/gift-cards/{id}/disable",
        action: Action::Settle,
        domain: "credit",
        query: None,
        summary: "Stop a gift card being spent",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/gift-cards/{id}/transactions",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "List what moved on a gift card",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/gift-cards/{id}/adjust",
        action: Action::Settle,
        domain: "credit",
        query: None,
        summary: "Correct a card's balance by hand, with a reason",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/customers/{id}/store-credit",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "Read a customer's balance in one currency",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/customers/{id}/store-credit",
        action: Action::Settle,
        domain: "credit",
        query: None,
        summary: "Put money on a customer's balance",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/store-credits/{id}/transactions",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "List what moved on a balance",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/store-credits/{id}/adjust",
        action: Action::Settle,
        domain: "credit",
        query: None,
        summary: "Correct a customer's balance by hand, with a reason",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/gift-cards",
        action: Action::Write,
        domain: "credit",
        query: None,
        summary: "Say this cart will pay with a gift card",
    },
    Route {
        surface: Surface::Store,
        method: Method::Post,
        path: "/store/carts/{id}/store-credit",
        action: Action::Write,
        domain: "credit",
        query: None,
        summary: "Say this cart will pay from my balance",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/carts/{id}/credits",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "What this cart means to pay with",
    },
    Route {
        surface: Surface::Store,
        method: Method::Delete,
        path: "/store/carts/{id}/credits/{credit_id}",
        action: Action::Write,
        domain: "credit",
        query: None,
        summary: "Take a gift card or a balance off this cart",
    },
    Route {
        surface: Surface::Store,
        method: Method::Get,
        path: "/store/customers/me/store-credit",
        action: Action::View,
        domain: "credit",
        query: None,
        summary: "Read my own balance in one currency",
    },
];
