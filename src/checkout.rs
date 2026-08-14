//! Turning a cart into an order, and undoing it cleanly when that fails.
//!
//! Six things have to happen and no transaction covers them: a promotion is
//! claimed, stock is reserved, an order is written, a provider is asked to hold
//! money, and the cart is closed. The provider is on the far side of a network,
//! so by the time it answers everything before it is committed and visible.
//!
//! This is therefore a [`Workflow`], and every step that writes says how to
//! undo itself:
//!
//! | Step | Does | Undoes by |
//! |---|---|---|
//! | `validate_cart` | reads | nothing — it wrote nothing |
//! | `claim_promotions` | [`promotion::claim`] | [`promotion::release`] |
//! | `reserve_stock` | [`inventory::reserve`] | [`inventory::release`] |
//! | `create_order` | order, lines, items at version 1, summary | deletes them |
//! | `authorize_payment` | [`payment::authorize`] | [`payment::cancel`] |
//! | `complete_cart` | stamps `completed_at` | clears it |
//!
//! **There is no capture here and there will not be one.** Taking the money is
//! a separate act with a separate permission, and it has no compensation: a
//! captured amount comes back as a refund, which is another provider call and
//! another row. A checkout that captured would have nothing honest to do when
//! the step after it failed.
//!
//! **A bank's second factor does not unwind anything.** When the provider
//! answers [`AuthorizationStatus::RequiresMore`] the session stays open, the
//! order stands at `requires_action`, and the step keeps nothing to compensate
//! with — the shopper comes back and the same run is driven again.
//!
//! # Running it twice
//!
//! The cart's `idempotency_key` is the workflow's transaction key. A second
//! request carrying the same key picks up the run the first one started rather
//! than starting a second, so two clicks on "place order" are one order.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::id::{
    CartId, CustomerId, LineItemId, OrderId, PaymentCollectionId, PaymentId, PaymentSessionId,
    PromotionId, ReservationId, ShippingOptionId, StockLocationId, VariantId,
};
use crate::money::{Currency, Money};
use crate::order::{
    self, NewAdjustment, NewOrder, NewOrderLine, NewOrderShipping, NewTaxLine, OrderAddress,
    OrderStatus, TaxSnapshot,
};
use crate::payment::{
    self, AuthorizationStatus, AuthorizeRequest, Authorized, NewCollection, NewSession,
    PaymentProvider, SessionRequest,
};
use crate::ports::{Action, AuditEntry, Ctx, Event, Permit, Resource, Tx};
use crate::workflow::{self, Failure, Outcome, Step, Workflow};
use crate::{cart, inventory, promotion};

/// What the run is holding as it goes, and what a compensation is handed back.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Carried {
    cart_id: Option<CartId>,
    customer_id: Option<CustomerId>,
    #[serde(default)]
    promotions: Vec<Claimed>,
    #[serde(default)]
    reservations: Vec<ReservationId>,
    order_id: Option<OrderId>,
    payment_collection_id: Option<PaymentCollectionId>,
    payment_session_id: Option<PaymentSessionId>,
    payment_id: Option<PaymentId>,
    #[serde(default)]
    requires_more: bool,
}

impl Carried {
    fn of(input: &Value) -> std::result::Result<Self, Failure> {
        serde_json::from_value(input.clone())
            .map_err(|_| Failure::Final(Error::bug("a checkout step was handed something else")))
    }

    fn cart(&self) -> std::result::Result<CartId, Failure> {
        self.cart_id
            .ok_or_else(|| Failure::Final(Error::invalid("a checkout needs a cart")))
    }

    fn value(&self) -> std::result::Result<Value, Failure> {
        serde_json::to_value(self)
            .map_err(|_| Failure::Final(Error::bug("a checkout step could not keep its place")))
    }
}

fn kept<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn recall<T: for<'de> Deserialize<'de> + Default>(kept: &Value) -> T {
    serde_json::from_value(kept.clone()).unwrap_or_default()
}

/// How this shop checks out: which provider holds the money, and which
/// location the stock comes off.
pub struct Checkout {
    provider: Arc<dyn PaymentProvider>,
    location_id: StockLocationId,
}

impl std::fmt::Debug for Checkout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Checkout")
            .field("provider", &self.provider.code())
            .field("location_id", &self.location_id)
            .finish()
    }
}

/// What a run came to.
#[derive(Debug, Clone)]
pub struct Placed {
    pub run: workflow::Run,
    pub order_id: Option<OrderId>,
    /// The provider wants the shopper to do something else first. Nothing was
    /// undone; driving the same key again picks up where this stopped.
    pub requires_more: bool,
}

impl Checkout {
    pub fn new(provider: Arc<dyn PaymentProvider>, location_id: StockLocationId) -> Self {
        Checkout {
            provider,
            location_id,
        }
    }

    /// The checkout with a step of the host's own on the end.
    ///
    /// The extra step runs after the cart is closed and unwinds everything
    /// before it when it fails, which is also how the tests reach the last
    /// compensation in the chain.
    pub fn workflow_then(&self, last: impl Step + 'static) -> Workflow {
        self.workflow().then(last)
    }

    pub fn workflow(&self) -> Workflow {
        Workflow::new("checkout")
            .then(ValidateCart)
            .then(ClaimPromotions)
            .then(ReserveStock {
                location_id: self.location_id,
            })
            .then(CreateOrder)
            .then(AuthorizePayment {
                provider: Arc::clone(&self.provider),
            })
            .then(CompleteCart)
    }

    /// Places the cart, or picks up the attempt its key already started.
    ///
    /// Takes the pool rather than a transaction: the engine opens one per step
    /// so a step that finished stays finished when a later one does not.
    pub async fn place(&self, pool: &PgPool, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Placed> {
        let _: Permit = ctx.permit(
            Action::Write,
            Resource::Cart {
                id: cart_id.as_uuid(),
                customer: None,
            },
        )?;

        let key = idempotency_key(pool, ctx, cart_id).await?;

        let run = workflow::run(
            pool,
            ctx,
            &self.workflow(),
            &key,
            serde_json::json!({ "cart_id": cart_id }),
        )
        .await?;

        let carried: Carried = recall(&run.output);

        Ok(Placed {
            order_id: carried.order_id,
            requires_more: carried.requires_more,
            run,
        })
    }
}

/// The key is the cart's, not the caller's: two requests for one cart have to
/// collide even when whoever sent them forgot to say so.
async fn idempotency_key(pool: &PgPool, ctx: &Ctx<'_>, cart_id: CartId) -> Result<String> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(ctx.scope.0.to_string())
        .execute(&mut *tx)
        .await?;

    let key: Option<Option<String>> =
        sqlx::query_scalar("select idempotency_key from cart where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(cart_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await?;
    tx.commit().await?;

    match key {
        None => Err(Error::not_found("cart")),
        Some(None) => Err(Error::invalid(
            "a cart needs an idempotency key before it is placed",
        )),
        Some(Some(key)) => Ok(key),
    }
}

// ---------------------------------------------------------------------------
// 1. Is the cart something that can be bought?
// ---------------------------------------------------------------------------

struct ValidateCart;

#[async_trait]
impl Step for ValidateCart {
    fn name(&self) -> &'static str {
        "validate_cart"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let carried = Carried::of(input)?;
        let cart_id = carried.cart()?;

        let cart = cart::get(tx, ctx, cart_id).await.map_err(Failure::Final)?;
        if cart.is_complete() {
            return Err(Failure::Final(Error::conflict(
                "that cart has already been ordered",
            )));
        }

        let lines = cart::lines(tx, ctx, cart_id)
            .await
            .map_err(Failure::Final)?;
        if lines.is_empty() {
            return Err(Failure::Final(Error::invalid("that cart is empty")));
        }

        for line in &lines {
            if line.currency_code != cart.currency_code {
                return Err(Failure::Final(Error::invalid(
                    "a line is priced in another currency",
                )));
            }
            if line.quantity <= 0 {
                return Err(Failure::Final(Error::invalid("a line has nothing on it")));
            }
        }

        if lines.iter().any(|line| line.requires_shipping) && cart.shipping_address_id.is_none() {
            return Err(Failure::Final(Error::invalid(
                "something on that cart has to be sent somewhere",
            )));
        }

        if cart.email.is_none() && cart.customer_id.is_none() {
            return Err(Failure::Final(Error::invalid(
                "an order needs somebody to belong to",
            )));
        }

        let carried = Carried {
            cart_id: Some(cart_id),
            customer_id: cart.customer_id,
            ..Carried::default()
        };

        Ok(Outcome::new(carried.value()?, Value::Null))
    }
}

// ---------------------------------------------------------------------------
// 2. Take the uses the promotions on this cart are owed
// ---------------------------------------------------------------------------

/// A promotion claimed, and the discount it gave away — a campaign with a
/// spend budget is charged that, and one counting uses ignores it.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Claimed {
    promotion: PromotionId,
    spend: Decimal,
    currency: String,
}

impl Claimed {
    fn money(&self) -> Result<Money> {
        Ok(Money::new(self.spend, Currency::parse(&self.currency)?))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ClaimedPromotions {
    promotions: Vec<Claimed>,
    customer_id: Option<CustomerId>,
}

struct ClaimPromotions;

#[async_trait]
impl Step for ClaimPromotions {
    fn name(&self) -> &'static str {
        "claim_promotions"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let cart_id = carried.cart()?;

        let currency: String =
            sqlx::query_scalar("select currency_code from cart where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(cart_id.as_uuid())
                .fetch_optional(&mut **tx)
                .await
                .map_err(|err| Failure::Retry(Error::from(err)))?
                .ok_or_else(|| Failure::Final(Error::not_found("cart")))?;

        let taken: Vec<(Uuid, Decimal)> = sqlx::query_as(
            "with taken as (
                 select a.promotion_id as id, a.amount as amount
                 from cart_line_item_adjustment a
                 join cart_line_item l on l.scope = a.scope and l.id = a.cart_line_item_id
                 where a.scope = $1 and l.cart_id = $2 and a.promotion_id is not null
                 union all
                 select a.promotion_id, a.amount
                 from cart_shipping_method_adjustment a
                 join cart_shipping_method m
                   on m.scope = a.scope and m.id = a.cart_shipping_method_id
                 where a.scope = $1 and m.cart_id = $2 and a.promotion_id is not null
             )
             select id, sum(amount) from taken group by id order by id",
        )
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .fetch_all(&mut **tx)
        .await
        .map_err(|err| Failure::Retry(Error::from(err)))?;

        for (id, spend) in taken {
            let claimed = Claimed {
                promotion: PromotionId::from_uuid(id),
                spend,
                currency: currency.clone(),
            };
            let money = claimed.money().map_err(Failure::Final)?;

            promotion::claim(tx, ctx, claimed.promotion, carried.customer_id, money)
                .await
                .map_err(Failure::Final)?;
            carried.promotions.push(claimed);
        }

        let undo = ClaimedPromotions {
            promotions: carried.promotions.clone(),
            customer_id: carried.customer_id,
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: ClaimedPromotions = recall(keep);

        for claimed in undo.promotions {
            let money = claimed.money()?;
            promotion::release(tx, ctx, claimed.promotion, undo.customer_id, money).await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 3. Promise the stock
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, FromRow)]
struct CartLine {
    id: LineItemId,
    variant_id: Option<Uuid>,
    product_id: Option<Uuid>,
    product_title: String,
    product_handle: Option<String>,
    variant_title: Option<String>,
    variant_sku: Option<String>,
    variant_option_values: Option<Value>,
    thumbnail: Option<String>,
    quantity: i32,
    unit_price: Decimal,
    compare_at_unit_price: Option<Decimal>,
    is_tax_inclusive: bool,
    is_discountable: bool,
    requires_shipping: bool,
}

#[derive(Debug, Clone, FromRow)]
struct CartMethod {
    id: Uuid,
    name: String,
    description: Option<String>,
    shipping_option_id: Option<Uuid>,
    amount: Decimal,
    is_tax_inclusive: bool,
    data: Option<Value>,
}

/// An adjustment or a tax line as the cart holds it, with the row it hangs off
/// so the order's copy can be hung off the matching one.
#[derive(Debug, Clone, FromRow)]
struct CartAdjustment {
    owner_id: Uuid,
    promotion_id: Option<Uuid>,
    code: Option<String>,
    amount: Decimal,
    description: Option<String>,
    is_tax_inclusive: bool,
    provider_id: Option<String>,
}

#[derive(Debug, Clone, FromRow)]
struct CartTaxLine {
    owner_id: Uuid,
    rate: Decimal,
    code: String,
    name: String,
    provider_id: Option<String>,
    description: Option<String>,
    treatment: Option<String>,
    jurisdiction_level: Option<String>,
    jurisdiction_code: Option<String>,
    jurisdiction_name: Option<String>,
    tax_code: Option<String>,
    provider: Option<String>,
    provider_transaction_id: Option<String>,
    calculated_at: Option<chrono::DateTime<chrono::Utc>>,
    address_country_code: Option<String>,
    address_province_code: Option<String>,
    address_postal_code: Option<String>,
    tax_id: Option<String>,
    tax_id_evidence: Option<String>,
    exemption_id: Option<Uuid>,
    evidence: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HeldStock {
    reservations: Vec<ReservationId>,
}

struct ReserveStock {
    location_id: StockLocationId,
}

#[async_trait]
impl Step for ReserveStock {
    fn name(&self) -> &'static str {
        "reserve_stock"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let cart_id = carried.cart()?;

        let lines = cart_lines(tx, ctx, cart_id).await.map_err(Failure::Final)?;

        for line in &lines {
            let Some(variant_id) = line.variant_id else {
                continue;
            };

            let items =
                inventory::inventory_items_for_variant(tx, ctx, VariantId::from_uuid(variant_id))
                    .await
                    .map_err(Failure::Final)?;

            for item in items {
                let held = inventory::reserve(
                    tx,
                    ctx,
                    item.inventory_item_id,
                    self.location_id,
                    line.quantity * item.required_quantity,
                    Some(line.id),
                    false,
                    None,
                )
                .await
                .map_err(Failure::Final)?;

                carried.reservations.push(held.id);
            }
        }

        let undo = HeldStock {
            reservations: carried.reservations.clone(),
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: HeldStock = recall(keep);

        for id in undo.reservations {
            inventory::release(tx, ctx, id).await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 4. Write the order
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WrittenOrder {
    order_id: Option<OrderId>,
}

struct CreateOrder;

#[async_trait]
impl Step for CreateOrder {
    fn name(&self) -> &'static str {
        "create_order"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;
        let cart_id = carried.cart()?;

        let cart = cart::get(tx, ctx, cart_id).await.map_err(Failure::Final)?;
        let currency = cart.currency().map_err(Failure::Final)?;

        let totals = cart::totals(tx, ctx, cart_id)
            .await
            .map_err(Failure::Final)?;

        let collection = payment::create_collection(
            tx,
            ctx,
            NewCollection {
                amount: totals.total,
                cart_id: Some(cart_id),
                metadata: Some(serde_json::json!({ "cart_id": cart_id })),
            },
        )
        .await
        .map_err(Failure::Final)?;

        let lines = cart_lines(tx, ctx, cart_id).await.map_err(Failure::Final)?;
        let methods = cart_methods(tx, ctx, cart_id)
            .await
            .map_err(Failure::Final)?;
        let (mut line_adjustments, mut line_taxes) = cart_money(tx, ctx, cart_id, false)
            .await
            .map_err(Failure::Final)?;
        let (mut method_adjustments, mut method_taxes) = cart_money(tx, ctx, cart_id, true)
            .await
            .map_err(Failure::Final)?;

        let new = NewOrder {
            region_id: cart.region_id,
            sales_channel_id: cart.sales_channel_id,
            customer_id: cart.customer_id,
            email: cart.email.clone(),
            currency_code: currency,
            locale: None,
            payment_collection_id: Some(collection.id),
            shipping_address: copy_address(tx, ctx, cart.shipping_address_id)
                .await
                .map_err(Failure::Final)?,
            billing_address: copy_address(tx, ctx, cart.billing_address_id)
                .await
                .map_err(Failure::Final)?,
            lines: lines
                .into_iter()
                .map(|line| NewOrderLine {
                    reserved_for: Some(line.id),
                    variant_id: line.variant_id.map(VariantId::from_uuid),
                    product_id: line.product_id,
                    title: line.product_title.clone(),
                    subtitle: line.variant_title.clone(),
                    thumbnail: line.thumbnail,
                    product_title: Some(line.product_title),
                    product_handle: line.product_handle,
                    variant_title: line.variant_title,
                    variant_sku: line.variant_sku,
                    variant_option_values: line.variant_option_values,
                    quantity: line.quantity,
                    unit_price: Money::new(line.unit_price, currency),
                    compare_at_unit_price: line.compare_at_unit_price,
                    is_tax_inclusive: line.is_tax_inclusive,
                    is_discountable: line.is_discountable,
                    requires_shipping: line.requires_shipping,
                    adjustments: line_adjustments
                        .remove(&line.id.as_uuid())
                        .unwrap_or_default(),
                    tax_lines: line_taxes.remove(&line.id.as_uuid()).unwrap_or_default(),
                    // A cart line carries no exemption, so a checkout sells
                    // nothing it claims is outside the withdrawal right.
                    withdrawal_exclusion: None,
                })
                .collect(),
            shipping: methods
                .into_iter()
                .map(|method| NewOrderShipping {
                    name: method.name,
                    description: method.description,
                    shipping_option_id: method.shipping_option_id.map(ShippingOptionId::from_uuid),
                    amount: Money::new(method.amount, currency),
                    is_tax_inclusive: method.is_tax_inclusive,
                    data: method.data,
                    adjustments: method_adjustments.remove(&method.id).unwrap_or_default(),
                    tax_lines: method_taxes.remove(&method.id).unwrap_or_default(),
                })
                .collect(),
            no_notification: None,
            metadata: Some(serde_json::json!({ "cart_id": cart_id })),
        };

        let placed = order::create(tx, ctx, new).await.map_err(Failure::Final)?;

        carried.order_id = Some(placed.id);
        carried.payment_collection_id = Some(collection.id);

        let undo = WrittenOrder {
            order_id: Some(placed.id),
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    /// The payment collection is left where it is: a session may already point
    /// at it by the time this runs, and an unused collection costs nothing
    /// while a dangling foreign key costs an incident.
    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: WrittenOrder = recall(keep);
        let Some(order_id) = undo.order_id else {
            return Ok(());
        };

        erase_order(tx, ctx, order_id).await
    }
}

// ---------------------------------------------------------------------------
// 5. Ask the provider to hold the money
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HeldMoney {
    payment_id: Option<PaymentId>,
    order_id: Option<OrderId>,
}

struct AuthorizePayment {
    provider: Arc<dyn PaymentProvider>,
}

#[async_trait]
impl Step for AuthorizePayment {
    fn name(&self) -> &'static str {
        "authorize_payment"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let mut carried = Carried::of(input)?;

        let collection_id = carried
            .payment_collection_id
            .ok_or_else(|| Failure::Final(Error::bug("a checkout lost its payment collection")))?;
        let order_id = carried
            .order_id
            .ok_or_else(|| Failure::Final(Error::bug("a checkout lost its order")))?;

        let collection = payment::collection(tx, ctx, collection_id)
            .await
            .map_err(Failure::Final)?;
        let owed = collection.total().map_err(Failure::Final)?;

        let session = payment::create_session(
            tx,
            ctx,
            NewSession {
                collection_id,
                provider_code: self.provider.code().to_string(),
                amount: owed,
                context: Some(serde_json::json!({ "order_id": order_id })),
                installment_count: None,
            },
        )
        .await
        .map_err(Failure::Final)?;

        let opened = self
            .provider
            .create_session(SessionRequest {
                session_id: session.id,
                collection_id,
                amount: owed,
                context: serde_json::json!({ "order_id": order_id }),
                account_holder: None,
            })
            .await
            .map_err(Failure::Retry)?;

        let session = payment::record_session(tx, ctx, session.id, opened)
            .await
            .map_err(Failure::Final)?;

        let answer = self
            .provider
            .authorize(AuthorizeRequest {
                session_id: session.id,
                amount: owed,
                data: session.data.clone(),
                context: serde_json::json!({ "order_id": order_id }),
                installment_count: session.installment_count,
            })
            .await
            .map_err(Failure::Retry)?;

        let requires_more = answer.status == AuthorizationStatus::RequiresMore;
        let authorized = payment::authorize(tx, ctx, session.id, answer)
            .await
            .map_err(Failure::Final)?;

        carried.payment_session_id = Some(session.id);

        match authorized {
            Authorized::Payment(paid) => {
                order::record_transaction(
                    tx,
                    ctx,
                    order_id,
                    Money::new(paid.amount, owed.currency),
                    "payment",
                    paid.id.as_uuid(),
                )
                .await
                .map_err(Failure::Final)?;

                carried.payment_id = Some(paid.id);

                let undo = HeldMoney {
                    payment_id: Some(paid.id),
                    order_id: Some(order_id),
                };

                Ok(Outcome::new(carried.value()?, kept(&undo)))
            }
            // Nothing is canceled and nothing is kept to compensate with: the
            // hold is still live and the shopper is expected back.
            Authorized::RequiresMore(_) if requires_more => {
                order::set_status(tx, ctx, order_id, OrderStatus::RequiresAction)
                    .await
                    .map_err(Failure::Final)?;

                carried.requires_more = true;

                Ok(Outcome::new(carried.value()?, Value::Null))
            }
            Authorized::RequiresMore(_) | Authorized::Failed(_) => Err(Failure::Final(
                Error::provider("payment", "the provider would not hold the money"),
            )),
        }
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: HeldMoney = recall(keep);
        let Some(payment_id) = undo.payment_id else {
            return Ok(());
        };

        payment::cancel(tx, ctx, payment_id).await?;

        // The ledger row this step wrote goes with it. An order's children are
        // `on delete restrict`, so leaving it here stops the earlier step from
        // undoing itself and the run fails rather than unwinding.
        if let Some(order_id) = undo.order_id {
            sqlx::query(
                "delete from order_transaction
                 where scope = $1 and order_id = $2 and reference = 'payment'
                   and reference_id = $3",
            )
            .bind(ctx.scope.0)
            .bind(order_id.as_uuid())
            .bind(payment_id.as_uuid())
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 6. Close the cart
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ClosedCart {
    cart_id: Option<CartId>,
}

struct CompleteCart;

#[async_trait]
impl Step for CompleteCart {
    fn name(&self) -> &'static str {
        "complete_cart"
    }

    async fn invoke(
        &self,
        tx: &mut Tx<'_>,
        ctx: &Ctx<'_>,
        input: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        let carried = Carried::of(input)?;
        let cart_id = carried.cart()?;

        let closed = sqlx::query(
            "update cart set completed_at = $3
             where scope = $1 and id = $2 and completed_at is null",
        )
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .bind(ctx.now())
        .execute(&mut **tx)
        .await
        .map_err(|err| Failure::Retry(Error::from(err)))?;

        if closed.rows_affected() == 0 {
            return Err(Failure::Final(Error::conflict(
                "that cart was closed by somebody else",
            )));
        }

        if let Some(order_id) = carried.order_id {
            ctx.audit(
                tx,
                AuditEntry {
                    actor: ctx.actor.clone(),
                    action: Action::Write,
                    entity: "order",
                    entity_id: order_id.as_uuid(),
                    summary: serde_json::json!({ "cart": cart_id }),
                },
            )
            .await
            .map_err(Failure::Final)?;

            ctx.emit(
                tx,
                Event {
                    name: "checkout.completed",
                    entity_id: order_id.as_uuid(),
                    payload: serde_json::json!({ "cart": cart_id }),
                },
            )
            .await
            .map_err(Failure::Final)?;
        }

        let undo = ClosedCart {
            cart_id: Some(cart_id),
        };

        Ok(Outcome::new(carried.value()?, kept(&undo)))
    }

    async fn compensate(&self, tx: &mut Tx<'_>, ctx: &Ctx<'_>, keep: &Value) -> Result<()> {
        let undo: ClosedCart = recall(keep);
        let Some(cart_id) = undo.cart_id else {
            return Ok(());
        };

        sqlx::query("update cart set completed_at = null where scope = $1 and id = $2")
            .bind(ctx.scope.0)
            .bind(cart_id.as_uuid())
            .execute(&mut **tx)
            .await?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Reading the cart, and taking an order back out again
// ---------------------------------------------------------------------------

async fn cart_lines(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Vec<CartLine>> {
    Ok(sqlx::query_as::<_, CartLine>(
        "select l.id, l.variant_id, l.product_id, l.product_title, l.product_handle,
                l.variant_title, l.variant_sku, l.variant_option_values, l.thumbnail,
                l.quantity, l.unit_price, l.compare_at_unit_price, l.is_tax_inclusive,
                l.is_discountable, l.requires_shipping
         from cart_line_item l
         where l.scope = $1 and l.cart_id = $2
         order by l.created_at, l.id",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?)
}

async fn cart_methods(tx: &mut Tx<'_>, ctx: &Ctx<'_>, cart_id: CartId) -> Result<Vec<CartMethod>> {
    Ok(sqlx::query_as::<_, CartMethod>(
        "select s.id, s.name, s.description, s.shipping_option_id, s.amount, s.is_tax_inclusive,
                s.data
         from cart_shipping_method s
         where s.scope = $1 and s.cart_id = $2
         order by s.created_at, s.id",
    )
    .bind(ctx.scope.0)
    .bind(cart_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?)
}

/// Everything the promotions and the tax engine put on a cart, keyed by the
/// line or the shipping method it belongs to. The order copies these rather
/// than blending them: one rate per row on the cart is one rate per row on the
/// order.
async fn cart_money(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    cart_id: CartId,
    shipping: bool,
) -> Result<(
    HashMap<Uuid, Vec<NewAdjustment>>,
    HashMap<Uuid, Vec<NewTaxLine>>,
)> {
    let (adjustment_query, tax_query) = if shipping {
        (
            "select a.cart_shipping_method_id as owner_id, a.promotion_id, a.code, a.amount,
                    a.description, a.is_tax_inclusive, a.provider_id
             from cart_shipping_method_adjustment a
             join cart_shipping_method s on s.scope = a.scope and s.id = a.cart_shipping_method_id
             where a.scope = $1 and s.cart_id = $2
             order by a.created_at, a.id",
            "select t.cart_shipping_method_id as owner_id, t.rate, t.code, t.name, t.provider_id,
                    t.description, t.treatment, t.jurisdiction_level, t.jurisdiction_code,
                    t.jurisdiction_name, t.tax_code, t.provider, t.provider_transaction_id,
                    t.calculated_at, t.address_country_code, t.address_province_code,
                    t.address_postal_code, t.tax_id, t.tax_id_evidence, t.exemption_id,
                    t.evidence
             from cart_shipping_method_tax_line t
             join cart_shipping_method s on s.scope = t.scope and s.id = t.cart_shipping_method_id
             where t.scope = $1 and s.cart_id = $2
             order by t.created_at, t.id",
        )
    } else {
        (
            "select a.cart_line_item_id as owner_id, a.promotion_id, a.code, a.amount,
                    a.description, a.is_tax_inclusive, a.provider_id
             from cart_line_item_adjustment a
             join cart_line_item l on l.scope = a.scope and l.id = a.cart_line_item_id
             where a.scope = $1 and l.cart_id = $2
             order by a.created_at, a.id",
            "select t.cart_line_item_id as owner_id, t.rate, t.code, t.name, t.provider_id,
                    t.description, t.treatment, t.jurisdiction_level, t.jurisdiction_code,
                    t.jurisdiction_name, t.tax_code, t.provider, t.provider_transaction_id,
                    t.calculated_at, t.address_country_code, t.address_province_code,
                    t.address_postal_code, t.tax_id, t.tax_id_evidence, t.exemption_id,
                    t.evidence
             from cart_line_item_tax_line t
             join cart_line_item l on l.scope = t.scope and l.id = t.cart_line_item_id
             where t.scope = $1 and l.cart_id = $2
             order by t.created_at, t.id",
        )
    };

    let adjustments = sqlx::query_as::<_, CartAdjustment>(adjustment_query)
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .fetch_all(&mut **tx)
        .await?;

    let taxes = sqlx::query_as::<_, CartTaxLine>(tax_query)
        .bind(ctx.scope.0)
        .bind(cart_id.as_uuid())
        .fetch_all(&mut **tx)
        .await?;

    let mut by_adjustment: HashMap<Uuid, Vec<NewAdjustment>> = HashMap::new();
    for row in adjustments {
        by_adjustment
            .entry(row.owner_id)
            .or_default()
            .push(NewAdjustment {
                promotion_id: row.promotion_id,
                code: row.code,
                amount: row.amount,
                description: row.description,
                is_tax_inclusive: row.is_tax_inclusive,
                provider_id: row.provider_id,
            });
    }

    let mut by_tax: HashMap<Uuid, Vec<NewTaxLine>> = HashMap::new();
    for row in taxes {
        by_tax.entry(row.owner_id).or_default().push(NewTaxLine {
            rate: row.rate,
            code: row.code,
            name: row.name,
            provider_id: row.provider_id,
            description: row.description,
            snapshot: TaxSnapshot {
                treatment: row.treatment,
                jurisdiction_level: row.jurisdiction_level,
                jurisdiction_code: row.jurisdiction_code,
                jurisdiction_name: row.jurisdiction_name,
                tax_code: row.tax_code,
                provider: row.provider,
                provider_transaction_id: row.provider_transaction_id,
                calculated_at: row.calculated_at,
                address_country_code: row.address_country_code,
                address_province_code: row.address_province_code,
                address_postal_code: row.address_postal_code,
                tax_id: row.tax_id,
                tax_id_evidence: row.tax_id_evidence,
                exemption_id: row.exemption_id,
                evidence: row.evidence,
            },
        });
    }

    Ok((by_adjustment, by_tax))
}

/// Copies the cart's address rather than pointing at it, which is the same
/// reason the cart copied it from the customer in the first place.
async fn copy_address(
    tx: &mut Tx<'_>,
    ctx: &Ctx<'_>,
    id: Option<Uuid>,
) -> Result<Option<OrderAddress>> {
    let Some(id) = id else {
        return Ok(None);
    };

    #[derive(FromRow)]
    struct Row {
        company: Option<String>,
        first_name: Option<String>,
        last_name: Option<String>,
        address_1: Option<String>,
        address_2: Option<String>,
        city: Option<String>,
        province: Option<String>,
        postal_code: Option<String>,
        country_code: Option<String>,
        phone: Option<String>,
    }

    let row = sqlx::query_as::<_, Row>(
        "select company, first_name, last_name, address_1, address_2, city, province,
                postal_code, country_code, phone
         from cart_address
         where scope = $1 and id = $2",
    )
    .bind(ctx.scope.0)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(row.map(|row| OrderAddress {
        company: row.company,
        first_name: row.first_name,
        last_name: row.last_name,
        address_1: row.address_1,
        address_2: row.address_2,
        city: row.city,
        province: row.province,
        postal_code: row.postal_code,
        country_code: row.country_code,
        phone: row.phone,
    }))
}

/// Takes an order back out, children first.
///
/// Only ever called to undo an order this run wrote moments ago. Every key
/// below is `on delete restrict`, so the children have to be named here and a
/// delete that hits something a later step added fails loudly rather than
/// tearing a live order apart. Nothing is reached by cascade on purpose: the
/// adjustments, tax lines, transfers and status history are the record of what
/// happened, and a compensation path is the last place that should quietly
/// take them.
async fn erase_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, order_id: OrderId) -> Result<()> {
    // The line money hangs off the line, not the order, and 0025 made it
    // `restrict` — so it goes before the line it belongs to.
    for (table, parent, key) in [
        (
            "order_line_item_adjustment",
            "order_line_item",
            "order_line_item_id",
        ),
        (
            "order_line_item_tax_line",
            "order_line_item",
            "order_line_item_id",
        ),
        (
            "order_shipping_method_adjustment",
            "order_shipping_method",
            "order_shipping_method_id",
        ),
        (
            "order_shipping_method_tax_line",
            "order_shipping_method",
            "order_shipping_method_id",
        ),
    ] {
        sqlx::query(&format!(
            "delete from {table}
             where scope = $1
               and {key} in (select id from {parent} where scope = $1 and order_id = $2)"
        ))
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    for table in [
        "order_status_history",
        "order_transfer",
        "order_summary",
        "order_item",
        "order_shipping_method",
        "order_credit_line",
        "order_transaction",
        "order_line_item",
    ] {
        sqlx::query(&format!(
            "delete from {table} where scope = $1 and order_id = $2"
        ))
        .bind(ctx.scope.0)
        .bind(order_id.as_uuid())
        .execute(&mut **tx)
        .await?;
    }

    let addresses: Vec<(Option<Uuid>, Option<Uuid>)> = sqlx::query_as(
        r#"delete from "order" where scope = $1 and id = $2
           returning shipping_address_id, billing_address_id"#,
    )
    .bind(ctx.scope.0)
    .bind(order_id.as_uuid())
    .fetch_all(&mut **tx)
    .await?;

    for (shipping, billing) in addresses {
        for address in [shipping, billing].into_iter().flatten() {
            sqlx::query("delete from order_address where scope = $1 and id = $2")
                .bind(ctx.scope.0)
                .bind(address)
                .execute(&mut **tx)
                .await?;
        }
    }

    ctx.audit(
        tx,
        AuditEntry {
            actor: ctx.actor.clone(),
            action: Action::Delete,
            entity: "order",
            entity_id: order_id.as_uuid(),
            summary: serde_json::json!({ "reason": "checkout compensated" }),
        },
    )
    .await?;

    Ok(())
}
