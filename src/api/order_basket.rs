//! The admin surface for `order_basket`: opening one, attaching what paid for
//! it, and a scope's own orders under it.
//!
//! There is no store-facing route here. A shopper never asks for a basket by
//! id — the checkout that opened it hands the order numbers back. What lives
//! here is what a back office, or a host's own split-checkout orchestration,
//! needs beyond an order it already reads: which other orders a basket
//! joined, and — before any order exists — which cart, this scope's own, is
//! the next leg of a checkout still in progress.
//!
//! [`list_carts`] is tagged `"cart"`, not `"order_basket"` — this file
//! already imports [`crate::cart`] and reaches for [`CartView`] on every
//! other route in it, so it is the smallest diff for the one list `cart`
//! itself offers, `cart::list`, to gain a route.

use serde::{Deserialize, Serialize};

use super::admin_order::OrderView;
use super::store::CartView;
use crate::cart;
use crate::error::Result;
use crate::id::{CustomerId, OrderBasketId, PaymentCollectionId};
use crate::money::Currency;
use crate::order_basket::{self, NewOrderBasket};
use crate::page::{Cursor, Page, Paging};
use crate::ports::{Action, Ctx, Tx};

use super::{Method, Route, Surface};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct BasketView {
    pub id: OrderBasketId,
    pub display_id: Option<i64>,
    pub customer_id: Option<CustomerId>,
    pub currency_code: String,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub email: Option<String>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<order_basket::OrderBasket> for BasketView {
    fn from(row: order_basket::OrderBasket) -> Self {
        BasketView {
            id: row.id,
            display_id: row.display_id,
            customer_id: row.customer_id,
            currency_code: row.currency_code,
            payment_collection_id: row.payment_collection_id,
            email: row.email,
            completed_at: row.completed_at,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenBasket {
    pub customer_id: Option<CustomerId>,
    pub currency_code: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

pub async fn open_basket(tx: &mut Tx<'_>, ctx: &Ctx<'_>, body: OpenBasket) -> Result<BasketView> {
    let new = NewOrderBasket {
        customer_id: body.customer_id,
        currency_code: Currency::parse(&body.currency_code)?,
        email: body.email,
        metadata: body.metadata,
    };

    Ok(BasketView::from(order_basket::open(tx, ctx, new).await?))
}

pub async fn get_basket(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderBasketId) -> Result<BasketView> {
    Ok(BasketView::from(order_basket::get(tx, ctx, id).await?))
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachPaymentCollection {
    pub payment_collection_id: PaymentCollectionId,
}

pub async fn attach_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderBasketId,
    body: AttachPaymentCollection,
) -> Result<BasketView> {
    Ok(BasketView::from(
        order_basket::attach_payment_collection(tx, ctx, id, body.payment_collection_id).await?,
    ))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListBasketOrders {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

impl ListBasketOrders {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match self.after.as_deref() {
            Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

/// This scope's own orders under one basket. Not the whole basket — a scope
/// can only ever read what it was seeded to read, and that is the point.
pub async fn basket_orders(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderBasketId,
    query: ListBasketOrders,
) -> Result<Page<OrderView>> {
    let page = order_basket::orders(tx, ctx, id, query.paging()?).await?;
    Ok(page.map(OrderView::from))
}

/// This scope's own carts under one basket — a host's split checkout finds
/// its next leg here: for each seller scope the basket touches, this is what
/// it calls, under that scope's own `Ctx`, to find the cart to place next
/// with `checkout::Machine::place`'s `basket_id`. Not the whole basket, the
/// same way `basket_orders` above is not: a scope reads what it was seeded
/// to read.
pub async fn basket_carts(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderBasketId,
    query: ListBasketOrders,
) -> Result<Page<CartView>> {
    let page = cart::for_basket(tx, ctx, id, query.paging()?).await?;
    Ok(page.map(CartView::from))
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListCarts {
    pub after: Option<String>,
    pub limit: Option<u32>,
    pub customer_id: Option<CustomerId>,
}

impl ListCarts {
    fn paging(&self) -> Result<Paging> {
        let limit = self.limit.unwrap_or(crate::page::DEFAULT_LIMIT);
        match self.after.as_deref() {
            Some(text) => Ok(Paging::after(Cursor::decode(text)?, limit)),
            None => Ok(Paging::first(limit)),
        }
    }
}

/// Every cart this scope holds, abandoned ones included — the back office's
/// own list, now that it has a screen for one. `cart::list` itself carried
/// this since before there was a route for it.
pub async fn list_carts(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    query: ListCarts,
) -> Result<Page<CartView>> {
    let page = cart::list(tx, ctx, query.customer_id, query.paging()?).await?;
    Ok(page.map(CartView::from))
}

pub(super) static ROUTES: &[Route] = &[
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/order-baskets",
        action: Action::Write,
        domain: "order_basket",
        query: None,
        summary: "Open a basket a checkout across seller scopes joins under",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/order-baskets/{id}",
        action: Action::View,
        domain: "order_basket",
        query: None,
        summary: "Read one basket",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Post,
        path: "/admin/order-baskets/{id}/payment-collection",
        action: Action::Write,
        domain: "order_basket",
        query: None,
        summary: "Say which collection paid for a basket",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/order-baskets/{id}/orders",
        action: Action::View,
        domain: "order_basket",
        query: None,
        summary: "This scope's own orders under a basket",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/order-baskets/{id}/carts",
        action: Action::View,
        domain: "order_basket",
        query: None,
        summary: "This scope's own carts under a basket, the next leg to place",
    },
    Route {
        surface: Surface::Admin,
        method: Method::Get,
        path: "/admin/carts",
        action: Action::View,
        domain: "cart",
        query: None,
        summary: "List carts, abandoned ones included",
    },
];
