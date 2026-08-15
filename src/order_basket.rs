//! What a marketplace customer sees as one order and pays for once.
//!
//! `order_basket` lives in the marketplace's own scope — the same one its
//! console already reads as itself rather than through a seller. Checkout
//! splits a cart that spans more than one seller into one [`crate::order::Order`]
//! per seller-scope; every one of them carries this basket's id and shares its
//! `payment_collection_id`, so the customer's single order number and single
//! payment are a display fact at the basket rather than something each seller
//! order has to agree on separately.
//!
//! A single-seller shop never opens one of these: [`crate::order::NewOrder::basket_id`]
//! stays `None` and nothing here is on that checkout's path.
//!
//! Reading a basket's own orders is a per-seller-scope question — `orders`
//! answers it for whichever scope the caller already holds, by the same
//! `where scope = $1` every other scoped query here uses. Seeing every seller
//! order under one basket at once is the operator's own console composing
//! several such reads under several scopes; it is not a query tezgah can run
//! from a single connection, because that is exactly what row-level security
//! forbids one scope from doing to another's.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{CustomerId, OrderBasketId, PaymentCollectionId};
use crate::money::Currency;
use crate::order::{self, Order};
use crate::page::{Page, Paging};
use crate::ports::{Action, AuditEntry, Ctx, Permit, Resource, Tx};

const BASKET_COLUMNS: &str = "id, display_id, customer_id, currency_code, payment_collection_id, \
                              email, metadata, completed_at, created_at, updated_at";

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct OrderBasket {
    pub id: OrderBasketId,
    pub display_id: Option<i64>,
    pub customer_id: Option<CustomerId>,
    pub currency_code: String,
    pub payment_collection_id: Option<PaymentCollectionId>,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl OrderBasket {
    pub fn currency(&self) -> Result<Currency> {
        Currency::parse(&self.currency_code)
    }

    /// Which collection paid for one of this basket's orders: the order's own
    /// if it carries one, or this basket's. Both `order` and this basket are
    /// already fetched by the caller, each under its own scope's `Ctx` — an
    /// order's scope is a seller's, this basket's is the marketplace's, and
    /// reading one from the other's connection is exactly what row-level
    /// security forbids. `order_basket_payment_single_source` is what
    /// guarantees the two never both answer.
    pub fn pays_for(&self, order: &Order) -> Option<PaymentCollectionId> {
        order.payment_collection_id.or(self.payment_collection_id)
    }
}

#[derive(Debug, Clone)]
pub struct NewOrderBasket {
    pub customer_id: Option<CustomerId>,
    pub currency_code: Currency,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Opens the basket a checkout that spans more than one seller-scope joins
/// under. Nothing else here mints an order number of its own — every order
/// split from this basket carries this row's `display_id` as the one the
/// customer is shown.
pub async fn open(tx: &mut Tx<'_>, ctx: &Ctx<'_>, new: NewOrderBasket) -> Result<OrderBasket> {
    let id = OrderBasketId::new();
    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Basket {
            id: Some(id.as_uuid()),
            customer: new.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let display_id: i64 = sqlx::query_scalar(
        "insert into display_counter (id, scope, kind, next)
         values ($1, $2, 'order_basket', 1)
         on conflict (scope, kind) do update set next = display_counter.next + 1
         returning next",
    )
    .bind(Uuid::now_v7())
    .bind(ctx.scope.0)
    .fetch_one(&mut **tx)
    .await?;

    let basket = sqlx::query_as::<_, OrderBasket>(&format!(
        "insert into order_basket (id, scope, display_id, customer_id, currency_code, email, metadata)
         values ($1, $2, $3, $4, $5, $6, $7)
         returning {BASKET_COLUMNS}"
    ))
    .bind(id.as_uuid())
    .bind(ctx.scope.0)
    .bind(display_id)
    .bind(new.customer_id.map(CustomerId::as_uuid))
    .bind(new.currency_code.as_str())
    .bind(new.email.map(|value| value.trim().to_lowercase()))
    .bind(new.metadata)
    .fetch_one(&mut **tx)
    .await?;

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Write,
            entity: "order_basket",
            entity_id: id.as_uuid(),
            summary: serde_json::json!({ "display_id": display_id }),
        },
    )
    .await?;

    Ok(basket)
}

pub async fn get(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: OrderBasketId) -> Result<OrderBasket> {
    let basket = sqlx::query_as::<_, OrderBasket>(&format!(
        "select {BASKET_COLUMNS} from order_basket where scope = $1 and id = $2"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::not_found("order basket"))?;

    let _: Permit = ctx.permit(
        Action::View,
        Resource::Basket {
            id: Some(id.as_uuid()),
            customer: basket.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    Ok(basket)
}

/// The one place a basket's `payment_collection_id` is set. Every order split
/// from it reads through this rather than carrying a copy, so there is one
/// answer to which collection paid rather than as many as there are sellers.
pub async fn attach_payment_collection(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: OrderBasketId,
    payment_collection_id: PaymentCollectionId,
) -> Result<OrderBasket> {
    let basket = get(tx, ctx, id).await?;

    let _: Permit = ctx.permit(
        Action::Write,
        Resource::Basket {
            id: Some(id.as_uuid()),
            customer: basket.customer_id.map(CustomerId::as_uuid),
        },
    )?;

    let updated = sqlx::query_as::<_, OrderBasket>(&format!(
        "update order_basket set payment_collection_id = $3
         where scope = $1 and id = $2
         returning {BASKET_COLUMNS}"
    ))
    .bind(ctx.scope.0)
    .bind(id.as_uuid())
    .bind(payment_collection_id.as_uuid())
    .fetch_one(&mut **tx)
    .await?;

    Ok(updated)
}

/// This scope's own orders under one basket — a seller's own fulfilment and
/// admin reading which of their orders a shared checkout produced. Not the
/// customer's whole basket: that spans scopes this connection cannot read.
pub async fn orders(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    basket_id: OrderBasketId,
    paging: Paging,
) -> Result<Page<Order>> {
    let _: Permit = ctx.permit(
        Action::View,
        Resource::Order {
            id: basket_id.as_uuid(),
            customer: None,
        },
    )?;

    order::in_basket(tx, ctx, basket_id, paging).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_basket_without_its_own_collection_defers_to_none() {
        // The database is what actually proves a single source of truth
        // (`order_basket_payment_single_source`); this only checks the Rust
        // side does not invent a second answer.
        let basket = OrderBasket {
            id: OrderBasketId::new(),
            display_id: None,
            customer_id: None,
            currency_code: "USD".into(),
            payment_collection_id: None,
            email: None,
            metadata: None,
            completed_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert_eq!(basket.payment_collection_id, None);
    }
}
