//! Checkout, against a real Postgres.
//!
//! The test that matters is `every_step_that_fails_leaves_nothing_behind`: the
//! run is broken at each step in turn and the same four questions are asked
//! afterwards every time — is any stock still reserved, is any money still
//! held, is there half an order, is the cart still open. A saga is only worth
//! having if the answers are no, no, no and yes.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::Shop;
use parking_lot::Mutex;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde_json::Value;
use sqlx::PgPool;
use tezgah::cart;
use tezgah::checkout::Checkout;
use tezgah::id::{CartId, OrderBasketId, StockLocationId, VariantId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, OrderStatus};
use tezgah::order_basket;
use tezgah::payment::{
    self, Authorization, AuthorizationStatus, AuthorizeRequest, CancelRequest, CaptureRequest,
    CaptureResult, PaymentProvider, RefundRequest, RefundResult, SessionRequest, SessionResponse,
    SessionStatus, WebhookEvent,
};
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};
use tezgah::subscription;
use tezgah::workflow::{Failure, Outcome, State, Step};
use tezgah::{Error, Result, inventory};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// A provider that answers however the test tells it to
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    Authorize,
    Decline,
    SecondFactor,
    /// `authorize` always fails, the way a provider that cannot be reached
    /// does — for reaching a `ReachingStep`'s `call`/`resume` failure path
    /// without ever producing an answer to record.
    NetworkDown,
}

struct Bank {
    answer: Mutex<Answer>,
    authorized: Mutex<u32>,
    canceled: Mutex<u32>,
}

impl Bank {
    fn saying(answer: Answer) -> Arc<Bank> {
        Arc::new(Bank {
            answer: Mutex::new(answer),
            authorized: Mutex::new(0),
            canceled: Mutex::new(0),
        })
    }
}

#[async_trait]
impl PaymentProvider for Bank {
    fn code(&self) -> &'static str {
        "bank"
    }

    async fn create_session(&self, _: SessionRequest) -> Result<SessionResponse> {
        Ok(SessionResponse {
            data: serde_json::json!({ "intent": "an-intent" }),
            status: SessionStatus::Pending,
        })
    }

    async fn authorize(&self, req: AuthorizeRequest) -> Result<Authorization> {
        *self.authorized.lock() += 1;

        if *self.answer.lock() == Answer::NetworkDown {
            return Err(Error::provider("bank", "could not be reached"));
        }

        let status = match *self.answer.lock() {
            Answer::Authorize => AuthorizationStatus::Authorized,
            Answer::Decline => AuthorizationStatus::Error,
            Answer::SecondFactor => AuthorizationStatus::RequiresMore,
            Answer::NetworkDown => unreachable!("handled above"),
        };

        Ok(Authorization {
            status,
            amount: Some(req.amount),
            data: serde_json::json!({ "intent": "an-intent" }),
            redirect: None,
            message: Some("the bank said so".into()),
            installment: None,
        })
    }

    async fn capture(&self, req: CaptureRequest) -> Result<CaptureResult> {
        Ok(CaptureResult {
            amount: req.amount,
            data: Value::Null,
        })
    }

    async fn refund(&self, req: RefundRequest) -> Result<RefundResult> {
        Ok(RefundResult {
            amount: req.amount,
            data: Value::Null,
        })
    }

    async fn cancel(&self, _: CancelRequest) -> Result<()> {
        *self.canceled.lock() += 1;
        Ok(())
    }

    fn parse_webhook(&self, _: &[(String, String)], _: &[u8]) -> Result<WebhookEvent> {
        Err(Error::invalid("this bank sends no webhooks"))
    }
}

/// A step that only ever fails, for reaching the compensation of the step
/// before it.
struct Collapse;

#[async_trait]
impl Step for Collapse {
    fn name(&self) -> &'static str {
        "collapse"
    }

    fn max_attempts(&self) -> i32 {
        1
    }

    async fn invoke(
        &self,
        _: &mut Tx<'_>,
        _: &Ctx<'_>,
        _: &Value,
    ) -> std::result::Result<Outcome, Failure> {
        Err(Failure::Final(Error::conflict("the sky fell in")))
    }
}

// ---------------------------------------------------------------------------
// A shop with something in a cart
// ---------------------------------------------------------------------------

struct Ready {
    cart_id: CartId,
    location_id: StockLocationId,
    variant_id: VariantId,
}

async fn seed_currency(tx: &mut Tx<'_>, scope: Scope) {
    sqlx::query(
        "insert into currency (id, scope, code, exponent, symbol, symbol_native, name)
         values ($1, $2, 'TRY', 2, 'x', 'x', 'Turkish lira')
         on conflict do nothing",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .execute(&mut **tx)
    .await
    .expect("a currency");
}

/// A cart of `quantity` at ten lira, an address on it, an idempotency key, and
/// `stocked` on the shelf.
async fn ready(shop: &Shop, stocked: i32, quantity: i32) -> Ready {
    ready_with(shop, stocked, quantity, true).await
}

/// The same, for something that is posted nowhere: the inventory item ships
/// nothing, and the only address on the cart is the one it is billed to.
async fn ready_with(shop: &Shop, stocked: i32, quantity: i32, ships: bool) -> Ready {
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, $3, true)",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind("bank")
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a thing".into()),
            requires_shipping: ships,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, stocked, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into cart_address (id, scope, address_1, city, country_code)
         values ($1, $2, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("an address");

    let cart = CartId::new();
    let column = if ships {
        "shipping_address_id"
    } else {
        "billing_address_id"
    };
    sqlx::query(&format!(
        "insert into cart
             (id, scope, currency_code, email, {column}, idempotency_key)
         values ($1, $2, 'TRY', 'shopper@example.com', $3, $4)"
    ))
    .bind(cart.as_uuid())
    .bind(shop.here.0)
    .bind(address)
    .bind(format!("key-{}", cart.as_uuid()))
    .execute(&mut *tx)
    .await
    .expect("a cart");

    if quantity > 0 {
        // Through the cart rather than by hand, so what the line says about
        // shipping is what the catalogue says rather than what a test wanted.
        cart::add_line(
            &mut tx,
            &ctx,
            cart,
            cart::AddLine {
                variant_id: variant,
                quantity,
                unit_price: Money::new(dec!(10), Currency::parse("TRY").expect("a currency")),
                is_tax_inclusive: false,
                selling_plan_id: None,
            },
        )
        .await
        .expect("a line");
    }

    tx.commit().await.expect("to commit the seed");

    Ready {
        cart_id: cart,
        location_id: location.id,
        variant_id: variant,
    }
}

// ---------------------------------------------------------------------------
// What "nothing left behind" means
// ---------------------------------------------------------------------------

async fn count(pool: &PgPool, scope: Scope, sql: &str) -> i64 {
    let mut tx = pool.begin().await.expect("a transaction");
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await
        .expect("to announce the scope");

    let value: i64 = sqlx::query_scalar(sql)
        .bind(scope.0)
        .fetch_one(&mut *tx)
        .await
        .expect("a count");
    tx.commit().await.expect("to commit");

    value
}

async fn nothing_left_behind(shop: &Shop, cart_id: CartId) {
    let scope = shop.here;

    assert_eq!(
        count(
            &shop.pool,
            scope,
            "select count(*) from reservation_item where scope = $1"
        )
        .await,
        0,
        "a reservation was left holding stock"
    );

    assert_eq!(
        count(
            &shop.pool,
            scope,
            "select coalesce(sum(reserved_quantity), 0)::bigint from inventory_level
             where scope = $1"
        )
        .await,
        0,
        "stock is still reserved for an order that was never placed"
    );

    assert_eq!(
        count(
            &shop.pool,
            scope,
            r#"select count(*) from "order" where scope = $1"#
        )
        .await,
        0,
        "half an order was left behind"
    );

    assert_eq!(
        count(
            &shop.pool,
            scope,
            "select count(*) from order_line_item where scope = $1"
        )
        .await,
        0,
        "an order's lines outlived the order"
    );

    assert_eq!(
        count(
            &shop.pool,
            scope,
            "select count(*) from payment where scope = $1 and canceled_at is null"
        )
        .await,
        0,
        "money is still being held"
    );

    assert_eq!(
        count(
            &shop.pool,
            scope,
            "select count(*) from promotion where scope = $1 and used > 0"
        )
        .await,
        0,
        "a promotion kept a use it never gave anybody"
    );

    let mut tx = shop.begin().await;
    let open: Option<Option<chrono::DateTime<chrono::Utc>>> =
        sqlx::query_scalar("select completed_at from cart where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(cart_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .expect("the cart");
    tx.commit().await.expect("to commit");

    assert_eq!(
        open.flatten(),
        None,
        "the cart was closed by a checkout that failed"
    );
}

// ---------------------------------------------------------------------------
// The happy path, so the rest means something
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_cart_becomes_an_order() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(
        placed.run.state,
        State::Done,
        "the checkout unwound instead of finishing: {:?}",
        placed.run.failure
    );
    assert!(!placed.requires_more);

    let order_id = placed.order_id.expect("an order");
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let order = order::get(&mut tx, &ctx, order_id).await?;
    assert_eq!(order.status()?, OrderStatus::Pending);
    assert_eq!(order.version, 1);
    assert_eq!(order.email.as_deref(), Some("shopper@example.com"));

    let totals = order::totals(&mut tx, &ctx, order_id, 1).await?;
    assert_eq!(totals.total.amount, dec!(20.00));

    let ledger = order::ledger(&mut tx, &ctx, order_id).await?;
    assert_eq!(ledger.authorized.amount, dec!(20.00));

    let items = order::items(&mut tx, &ctx, order_id, 1).await?;
    assert_eq!(items.first().map(|item| item.quantity), Some(2));

    // The stock is promised, not gone: nothing has shipped.
    let item = inventory::inventory_items_for_variant(&mut tx, &ctx, here.variant_id)
        .await?
        .first()
        .expect("an inventory item")
        .inventory_item_id;
    let level = inventory::level(&mut tx, &ctx, item, here.location_id).await?;
    assert_eq!(level.reserved_quantity, 2);
    assert_eq!(level.stocked_quantity, 10);

    let closed: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("select completed_at from cart where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(here.cart_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
    assert!(closed.is_some());

    tx.commit().await.expect("to commit");
    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Break it at every step in turn
// ---------------------------------------------------------------------------

#[tokio::test]
async fn validate_cart_failing_leaves_nothing_behind() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 0).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);
    assert!(placed.order_id.is_none());

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn claim_promotions_failing_leaves_nothing_behind() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;

    // A promotion with no uses left, already on the cart's only line.
    let mut tx = shop.begin().await;
    let promotion = Uuid::now_v7();
    sqlx::query(
        "insert into promotion (id, scope, code, type, status, usage_limit, used)
         values ($1, $2, 'SPENT', 'standard', 'active', 1, 1)",
    )
    .bind(promotion)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a spent promotion");

    sqlx::query(
        "insert into cart_line_item_adjustment
             (id, scope, cart_line_item_id, promotion_id, code, amount, currency_code)
         select $1, $2, l.id, $3, 'SPENT', 1, 'TRY'
         from cart_line_item l where l.scope = $2 and l.cart_id = $4",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(promotion)
    .bind(here.cart_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("an adjustment");
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    // The promotion is back to the one use it had before, not two.
    assert_eq!(
        count(
            &shop.pool,
            shop.here,
            "select count(*) from promotion where scope = $1 and used <> 1"
        )
        .await,
        0
    );

    let mut tx = shop.begin().await;
    let open: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("select completed_at from cart where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(here.cart_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
    assert!(open.is_none());
    tx.commit().await.expect("to commit");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn reserve_stock_failing_leaves_nothing_behind() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 1, 2).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn create_order_failing_leaves_nothing_behind() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;

    // A discount larger than the cart, so the total is below nothing and the
    // payment collection refuses it. No promotion, so nothing is claimed.
    let mut tx = shop.begin().await;
    sqlx::query(
        "insert into cart_line_item_adjustment
             (id, scope, cart_line_item_id, amount, currency_code)
         select $1, $2, l.id, 1000, 'TRY'
         from cart_line_item l where l.scope = $2 and l.cart_id = $3",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(here.cart_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("an adjustment");
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    nothing_left_behind(&shop, here.cart_id).await;
    assert_eq!(*bank.authorized.lock(), 0, "the bank was never asked");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_declined_card_leaves_nothing_behind() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::Decline);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

/// #158 §4: `prepare` opens the session and commits on its own, before the
/// provider is ever asked. When the provider cannot be reached at all — every
/// `call`/`resume` failing — the run never finishes, but the session
/// `prepare` opened must not be left open once attempts run out and the run
/// gives up: `compensate` is fed from `prepare`, not only from `record`.
///
/// `AuthorizePayment` does not override `max_attempts`, so three calls with
/// the same idempotency key — what a client retrying a failed request looks
/// like — is what it takes to exhaust it: the first two find the row
/// `called` from the one before and resume it, asking the provider again
/// because `Bank` answers no `LookupProvider`; the third gives up.
#[tokio::test]
async fn a_session_the_provider_never_answered_is_closed_once_attempts_are_gone() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::NetworkDown);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let first = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await;
    assert!(
        first.is_err(),
        "the first attempt should report the run is not finished yet"
    );
    let second = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await;
    assert!(second.is_err(), "the second attempt should be the same");

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(
        placed.run.state,
        State::Reverted,
        "{:?}",
        placed.run.failure
    );
    assert_eq!(
        *bank.authorized.lock(),
        3,
        "the provider should have been asked once per attempt"
    );

    nothing_left_behind(&shop, here.cart_id).await;

    let open_sessions = count(
        &shop.pool,
        shop.here,
        "select count(*) from payment_session
         where scope = $1 and status in ('pending', 'requires_more')",
    )
    .await;
    assert_eq!(
        open_sessions, 0,
        "the session prepare opened outlived the run that opened it"
    );

    shop.close().await;
    Ok(())
}

/// The last compensation in the chain, reached by putting a step that always
/// fails after the cart has been closed.
#[tokio::test]
async fn complete_cart_is_undone_when_something_after_it_fails() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let run = tezgah::workflow::run(
        &shop.pool,
        &shop.ctx(),
        &checkout.workflow_then(Collapse),
        "one-that-falls-over",
        serde_json::json!({ "cart_id": here.cart_id }),
    )
    .await?;

    let mut look = shop.begin().await;
    let stuck: Vec<(String, String)> =
        sqlx::query_as("select step_name, failure from workflow_dead_letter where run_id = $1")
            .bind(run.id.as_uuid())
            .fetch_all(&mut *look)
            .await?;
    look.commit().await?;

    assert_eq!(
        run.state,
        State::Reverted,
        "the run did not unwind: {:?}; compensations that failed: {stuck:?}",
        run.failure
    );
    assert_eq!(*bank.authorized.lock(), 1);

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Twice is once, and a second factor is not a failure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn one_key_makes_one_order() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let first = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    let second = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;

    assert_eq!(first.order_id, second.order_id);
    assert_eq!(first.run.id, second.run.id);
    assert_eq!(
        count(
            &shop.pool,
            shop.here,
            r#"select count(*) from "order" where scope = $1"#
        )
        .await,
        1,
        "one cart and one key became two orders"
    );
    assert_eq!(
        *bank.authorized.lock(),
        1,
        "the bank was asked to hold the money twice"
    );

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_second_factor_keeps_everything_it_has() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::SecondFactor);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    // A run held open by a second factor has not finished: the shopper is
    // still expected to come back and the run has to be resumable when they do.
    assert_eq!(placed.run.state, State::Waiting);
    assert!(placed.requires_more);

    let order_id = placed.order_id.expect("an order that is waiting");
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let order = order::get(&mut tx, &ctx, order_id).await?;
    assert_eq!(order.status()?, OrderStatus::RequiresAction);
    tx.commit().await.expect("to commit");

    // Nothing was cancelled and nothing was given back: the shopper is
    // expected to come back to it.
    assert_eq!(*bank.canceled.lock(), 0);
    assert_eq!(
        count(
            &shop.pool,
            shop.here,
            "select count(*) from reservation_item where scope = $1"
        )
        .await,
        1,
        "the stock was let go of while the shopper was at their bank"
    );

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_without_a_key_is_not_placed() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    sqlx::query("update cart set idempotency_key = null where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(here.cart_id.as_uuid())
        .execute(&mut *tx)
        .await?;
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let refused = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await
        .expect_err("a cart with no key is not placed");
    assert_eq!(refused.code(), "invalid");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_cart_in_another_scope_is_not_reachable() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let refused = checkout
        .place(&shop.pool, &shop.theirs(), here.cart_id, None)
        .await
        .expect_err("somebody else's cart is not there");
    assert!(refused.is_not_found());

    shop.close().await;
    Ok(())
}

/// The parts of a total have to add back up to it whatever the checkout did to
/// get there, and the order's own sum is the cart's.
#[tokio::test]
async fn the_order_is_worth_what_the_cart_was() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 3).await;

    let mut tx = shop.begin().await;
    sqlx::query(
        "insert into cart_line_item_tax_line
             (id, scope, cart_line_item_id, rate, code, name)
         select $1, $2, l.id, 18, 'kdv', 'KDV'
         from cart_line_item l where l.scope = $2 and l.cart_id = $3",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(here.cart_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("a tax line");

    let before = tezgah::cart::totals(&mut tx, &shop.ctx(), here.cart_id).await?;
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    let order_id = placed.order_id.expect("an order");

    let mut tx = shop.begin().await;
    let after = order::totals(&mut tx, &shop.ctx(), order_id, 1).await?;
    tx.commit().await.expect("to commit");

    assert_eq!(before, after);
    assert_eq!(
        after.total.amount,
        after.subtotal.amount - after.discount.amount + after.shipping.amount + after.tax.amount
    );
    assert!(after.tax.amount > Decimal::ZERO);

    shop.close().await;
    Ok(())
}

/// A currency mismatch is the caller's mistake, not the provider's, so it is
/// refused before anything is reserved.
#[tokio::test]
async fn a_line_in_the_wrong_currency_stops_at_the_gate() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    sqlx::query(
        "update cart_line_item set currency_code = 'EUR' where scope = $1 and cart_id = $2",
    )
    .bind(shop.here.0)
    .bind(here.cart_id.as_uuid())
    .execute(&mut *tx)
    .await?;
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    nothing_left_behind(&shop, here.cart_id).await;
    assert_eq!(*bank.authorized.lock(), 0);

    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// A campaign's money
// ---------------------------------------------------------------------------

/// A promotion on a campaign whose budget is money, taking `discount` off the
/// cart's only line.
async fn a_spending_promotion(shop: &Shop, cart_id: CartId, cap: Decimal, discount: Decimal) {
    let mut tx = shop.begin().await;

    let campaign = Uuid::now_v7();
    sqlx::query("insert into campaign (id, scope, identifier, name) values ($1, $2, $3, $3)")
        .bind(campaign)
        .bind(shop.here.0)
        .bind(format!("camp-{campaign}"))
        .execute(&mut *tx)
        .await
        .expect("a campaign");

    sqlx::query(
        r#"insert into campaign_budget (id, scope, campaign_id, type, "limit", currency_code)
           values ($1, $2, $3, 'spend', $4, 'TRY')"#,
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(campaign)
    .bind(cap)
    .execute(&mut *tx)
    .await
    .expect("a spend budget");

    let promotion = Uuid::now_v7();
    sqlx::query(
        "insert into promotion (id, scope, code, type, status, campaign_id)
         values ($1, $2, $3, 'standard', 'active', $4)",
    )
    .bind(promotion)
    .bind(shop.here.0)
    .bind(format!("SPEND-{promotion}"))
    .bind(campaign)
    .execute(&mut *tx)
    .await
    .expect("a promotion");

    sqlx::query(
        "insert into cart_line_item_adjustment
             (id, scope, cart_line_item_id, promotion_id, code, amount, currency_code)
         select $1, $2, l.id, $3, 'SPEND', $5, 'TRY'
         from cart_line_item l where l.scope = $2 and l.cart_id = $4",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(promotion)
    .bind(cart_id.as_uuid())
    .bind(discount)
    .execute(&mut *tx)
    .await
    .expect("an adjustment");

    tx.commit().await.expect("to commit the campaign");
}

async fn budget_used(shop: &Shop) -> Decimal {
    let mut tx = shop.begin().await;
    let used: Decimal = sqlx::query_scalar("select used from campaign_budget where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the budget");
    tx.commit().await.expect("to commit");
    used
}

#[tokio::test]
async fn a_checkout_that_would_overspend_a_campaign_is_refused() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    a_spending_promotion(&shop, here.cart_id, dec!(5.00), dec!(6.00)).await;

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;

    assert_eq!(placed.run.state, State::Reverted);
    assert_eq!(budget_used(&shop).await, Decimal::ZERO);

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_checkout_charges_a_campaigns_money_and_gives_it_back_when_it_unwinds() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    a_spending_promotion(&shop, here.cart_id, dec!(20.00), dec!(6.00)).await;

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let run = tezgah::workflow::run(
        &shop.pool,
        &shop.ctx(),
        &checkout.workflow_then(Collapse),
        "one-that-spends-and-falls-over",
        serde_json::json!({ "cart_id": here.cart_id }),
    )
    .await?;

    assert_eq!(run.state, State::Reverted);
    assert_eq!(
        budget_used(&shop).await,
        Decimal::ZERO,
        "the campaign kept money a checkout that never happened gave away"
    );

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

/// What the promotions and the tax engine put on the cart is copied onto the
/// order a row at a time: two rates stay two rows, and the promotion that gave
/// the discount is still named on the order's own adjustment.
#[tokio::test]
async fn the_order_copies_the_cart_s_adjustments_and_rates() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let promotion = Uuid::now_v7();

    let mut tx = shop.begin().await;
    sqlx::query(
        "insert into promotion (id, scope, code, type, status, usage_limit, used)
         values ($1, $2, 'COPIED', 'standard', 'active', 10, 0)",
    )
    .bind(promotion)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a promotion with uses left");

    for (rate, code, name) in [(dec!(18), "kdv", "KDV"), (dec!(8), "city", "City levy")] {
        sqlx::query(
            "insert into cart_line_item_tax_line
                 (id, scope, cart_line_item_id, rate, code, name)
             select $1, $2, l.id, $4, $5, $6
             from cart_line_item l where l.scope = $2 and l.cart_id = $3",
        )
        .bind(Uuid::now_v7())
        .bind(shop.here.0)
        .bind(here.cart_id.as_uuid())
        .bind(rate)
        .bind(code)
        .bind(name)
        .execute(&mut *tx)
        .await
        .expect("a tax line");
    }

    sqlx::query(
        "insert into cart_line_item_adjustment
             (id, scope, cart_line_item_id, promotion_id, code, amount, currency_code)
         select $1, $2, l.id, $4, 'SUMMER', 3, 'TRY'
         from cart_line_item l where l.scope = $2 and l.cart_id = $3",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(here.cart_id.as_uuid())
    .bind(promotion)
    .execute(&mut *tx)
    .await
    .expect("an adjustment");

    let before = tezgah::cart::totals(&mut tx, &shop.ctx(), here.cart_id).await?;
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    let order_id = placed.order_id.unwrap_or_else(|| {
        panic!(
            "the checkout did not place an order: {:?} {:?}",
            placed.run.state, placed.run.failure
        )
    });

    let mut tx = shop.begin().await;

    let rates: Vec<Decimal> = sqlx::query_scalar(
        "select t.rate from order_line_item_tax_line t
         join order_line_item l on l.scope = t.scope and l.id = t.order_line_item_id
         where t.scope = $1 and l.order_id = $2
         order by t.rate",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .expect("the order's tax lines");
    assert_eq!(rates, vec![dec!(8), dec!(18)], "the rates were blended");

    let (amount, from): (Decimal, Option<Uuid>) = sqlx::query_as(
        "select a.amount, a.promotion_id from order_line_item_adjustment a
         join order_line_item l on l.scope = a.scope and l.id = a.order_line_item_id
         where a.scope = $1 and l.order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the order's adjustment");
    assert_eq!(amount, dec!(3));
    assert_eq!(from, Some(promotion), "the promotion was lost on the way");

    let after = order::totals(&mut tx, &shop.ctx(), order_id, 1).await?;
    assert_eq!(before, after);
    tx.commit().await.expect("to commit");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_gift_card_is_sold_without_a_shipping_address() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready_with(&shop, 10, 1, false).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let lines = cart::lines(&mut tx, &ctx, here.cart_id).await?;
    assert!(
        !lines[0].requires_shipping,
        "a variant whose inventory item ships nothing still asked for a courier"
    );
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(
        placed.run.state,
        State::Done,
        "a cart that ships nothing was refused for having nowhere to ship to: {:?}",
        placed.run.failure
    );
    assert!(placed.order_id.is_some());

    shop.close().await;
    Ok(())
}

/// The variant says it is a gift card, and everything downstream follows: the
/// cart is not taxed on it, the order line says so, and the total is the face
/// value rather than the face value plus VAT.
#[tokio::test]
async fn a_gift_card_variant_is_sold_untaxed_and_says_so() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let region = Uuid::now_v7();
    sqlx::query("insert into tax_region (id, scope, country_code) values ($1, $2, 'TR')")
        .bind(region)
        .bind(shop.here.0)
        .execute(&mut *tx)
        .await
        .expect("a tax region");
    sqlx::query(
        "insert into tax_rate (id, scope, tax_region_id, rate, code, name, is_default)
         values ($1, $2, $3, 20, 'kdv', 'KDV', true)",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(region)
    .execute(&mut *tx)
    .await
    .expect("a rate");

    // Untouched, the basket is taxed like anything else.
    tezgah::api::store::reprice(&mut tx, &ctx, here.cart_id).await?;
    let taxed = cart_tax_lines(&mut tx, &shop, here.cart_id).await;
    assert_eq!(
        taxed, 1,
        "an ordinary line was not taxed, so nothing is proven"
    );

    sqlx::query("update product_variant set is_giftcard = true where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(here.variant_id.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the variant to become a gift card");

    tezgah::api::store::reprice(&mut tx, &ctx, here.cart_id).await?;
    assert_eq!(
        cart_tax_lines(&mut tx, &shop, here.cart_id).await,
        0,
        "selling a gift card is money changing form, not a taxable supply"
    );

    let totals = cart::totals(&mut tx, &ctx, here.cart_id).await?;
    assert_eq!(totals.tax.amount, Decimal::ZERO);
    assert_eq!(totals.total.amount, dec!(20));
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    let order_id = placed.order_id.unwrap_or_else(|| {
        panic!(
            "the checkout did not place an order: {:?} {:?}",
            placed.run.state, placed.run.failure
        )
    });

    let mut tx = shop.begin().await;
    let lines = order::line_items(&mut tx, &shop.ctx(), order_id).await?;
    assert!(lines[0].is_giftcard, "the line could not say what it was");

    let rates: i64 = sqlx::query_scalar(
        "select count(*) from order_line_item_tax_line t
         join order_line_item l on l.scope = t.scope and l.id = t.order_line_item_id
         where t.scope = $1 and l.order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order_id.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the order's tax lines");
    assert_eq!(rates, 0);

    let totals = order::totals(&mut tx, &shop.ctx(), order_id, 1).await?;
    assert_eq!(totals.tax.amount, Decimal::ZERO);
    assert_eq!(totals.total.amount, dec!(20));
    tx.commit().await.expect("to commit");

    shop.close().await;
    Ok(())
}

async fn cart_tax_lines(tx: &mut Tx<'_>, shop: &Shop, cart_id: CartId) -> i64 {
    sqlx::query_scalar(
        "select count(*) from cart_line_item_tax_line t
         join cart_line_item l on l.scope = t.scope and l.id = t.cart_line_item_id
         where t.scope = $1 and l.cart_id = $2",
    )
    .bind(shop.here.0)
    .bind(cart_id.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the cart's tax lines")
}

/// The whole point of printing a card: somebody spends it.
#[tokio::test]
async fn a_purchased_gift_card_pays_for_the_next_basket() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready(&shop, 10, 2).await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let lira = Currency::parse("TRY").expect("a currency");
    let sold = order::create(
        &mut tx,
        &ctx,
        order::NewOrder {
            lines: vec![order::NewOrderLine {
                selling_plan_id: None,
                is_giftcard: true,
                ..order::NewOrderLine::of("A gift card", 1, Money::new(dec!(50), lira))
            }],
            ..order::NewOrder::of(lira)
        },
    )
    .await?;

    order::record_transaction(
        &mut tx,
        &ctx,
        sold.id,
        Money::new(dec!(50), lira),
        "capture",
        Uuid::now_v7(),
    )
    .await?;

    let printed = tezgah::credit::issue_purchased(&mut tx, &ctx, sold.id).await?;
    assert_eq!(printed.len(), 1);

    let code = shop
        .host
        .payloads_of("gift_card.purchased")
        .first()
        .and_then(|payload| payload["code"].as_str().map(str::to_owned))
        .expect("the code the host has to post to the buyer");

    tezgah::credit::apply_gift_card(
        &mut tx,
        &ctx,
        here.cart_id,
        &code,
        Money::new(dec!(20), lira),
    )
    .await?;
    tx.commit().await.expect("to commit");

    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Done, "{:?}", placed.run.failure);

    let mut tx = shop.begin().await;
    let card = tezgah::credit::gift_card(&mut tx, &ctx, printed[0]).await?;
    assert_eq!(
        card.balance,
        dec!(30),
        "the basket did not come off the card"
    );
    tx.commit().await.expect("to commit");

    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Subscribing from the storefront
// ---------------------------------------------------------------------------

struct SubscribeReady {
    cart_id: CartId,
    location_id: StockLocationId,
}

/// A cart holding one line sold on a plan, for a signed-in customer who
/// already has an instrument on file with `bank`.
async fn ready_to_subscribe(shop: &Shop) -> SubscribeReady {
    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    seed_currency(&mut tx, shop.here).await;

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, 'bank', true)",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a monthly thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 10, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(shop.here.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(shop.here.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let customer = common::a_customer(&mut tx, &ctx).await;

    payment::save_account_holder(
        &mut tx,
        &ctx,
        payment::NewAccountHolder {
            provider_code: "bank".into(),
            customer_id: Some(customer),
            external_id: format!("cus_{}", Uuid::now_v7().simple()),
            email: None,
            data: serde_json::json!({}),
        },
    )
    .await
    .expect("an account holder");

    let group = subscription::create_plan_group(
        &mut tx,
        &ctx,
        subscription::NewPlanGroup {
            name: "Monthly".into(),
            ..subscription::NewPlanGroup::default()
        },
    )
    .await
    .expect("a group");

    let plan = subscription::create_plan(
        &mut tx,
        &ctx,
        group.id,
        subscription::NewPlan {
            name: "Every month".into(),
            billing_interval_unit: "month".into(),
            billing_interval_count: 1,
            ..subscription::NewPlan::default()
        },
    )
    .await
    .expect("a plan");

    subscription::attach_variant(&mut tx, &ctx, plan.id, variant)
        .await
        .expect("the variant to be sold on it");

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into cart_address (id, scope, address_1, city, country_code)
         values ($1, $2, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(shop.here.0)
    .execute(&mut *tx)
    .await
    .expect("an address");

    let cart_id = CartId::new();
    sqlx::query(
        "insert into cart
             (id, scope, customer_id, currency_code, email, shipping_address_id, idempotency_key)
         values ($1, $2, $3, 'TRY', 'shopper@example.com', $4, $5)",
    )
    .bind(cart_id.as_uuid())
    .bind(shop.here.0)
    .bind(customer.as_uuid())
    .bind(address)
    .bind(format!("key-{}", cart_id.as_uuid()))
    .execute(&mut *tx)
    .await
    .expect("a cart");

    cart::add_line(
        &mut tx,
        &ctx,
        cart_id,
        cart::AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: Money::new(dec!(10), Currency::parse("TRY").expect("a currency")),
            is_tax_inclusive: false,
            selling_plan_id: Some(plan.id),
        },
    )
    .await
    .expect("a line sold on a plan");

    tx.commit().await.expect("to commit the seed");

    SubscribeReady {
        cart_id,
        location_id: location.id,
    }
}

#[tokio::test]
async fn subscribing_from_the_storefront_opens_a_contract() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready_to_subscribe(&shop).await;
    let bank = Bank::saying(Answer::Authorize);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Done, "{:?}", placed.run.failure);

    let count: i64 = {
        let mut tx = shop.begin().await;
        let value: i64 = sqlx::query_scalar("select count(*) from subscription where scope = $1")
            .bind(shop.here.0)
            .fetch_one(&mut *tx)
            .await
            .expect("to count contracts");
        tx.commit().await.expect("to commit");
        value
    };
    assert_eq!(count, 1, "one contract opened for the line sold on a plan");

    shop.close().await;
    Ok(())
}

#[tokio::test]
async fn a_declined_card_rolls_back_the_contract_it_would_have_opened() -> Result<()> {
    let shop = Shop::open().await;
    let here = ready_to_subscribe(&shop).await;
    let bank = Bank::saying(Answer::Decline);
    let checkout = Checkout::new(
        Arc::clone(&bank) as Arc<dyn PaymentProvider>,
        here.location_id,
    );

    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id, None)
        .await?;
    assert_eq!(placed.run.state, State::Reverted);

    let count: i64 = {
        let mut tx = shop.begin().await;
        let value: i64 = sqlx::query_scalar("select count(*) from subscription where scope = $1")
            .bind(shop.here.0)
            .fetch_one(&mut *tx)
            .await
            .expect("to count contracts");
        tx.commit().await.expect("to commit");
        value
    };
    assert_eq!(count, 0, "a declined card leaves no contract behind");

    nothing_left_behind(&shop, here.cart_id).await;
    shop.close().await;
    Ok(())
}

// ---------------------------------------------------------------------------
// A checkout that spans two seller scopes
// ---------------------------------------------------------------------------

/// A basket, opened under its own (marketplace) scope, and one ready-to-place
/// cart per seller scope in `sellers`, each carrying that basket's id — the
/// shape `README.md` describes a host assembling: one scope per seller,
/// joined by a basket only the marketplace's own scope can open.
async fn basket_with_legs(shop: &Shop, sellers: &[Scope]) -> (Scope, OrderBasketId, Vec<Ready>) {
    let marketplace = Scope(Uuid::now_v7());
    sqlx::query("insert into tezgah_scope (id) values ($1)")
        .bind(marketplace.0)
        .execute(&shop.pool)
        .await
        .expect("a marketplace scope");
    let marketplace_ctx = Ctx::new(marketplace, Actor::System, shop.host.as_ref() as &dyn Host);

    let basket = {
        let mut tx = shop.begin_as(marketplace).await;
        let basket = order_basket::open(
            &mut tx,
            &marketplace_ctx,
            order_basket::NewOrderBasket {
                customer_id: None,
                currency_code: Currency::parse("TRY").expect("a currency"),
                email: Some("shopper@example.com".into()),
                metadata: None,
            },
        )
        .await
        .expect("a basket");
        tx.commit().await.expect("to commit the basket");
        basket.id
    };

    let mut legs = Vec::new();
    for &scope in sellers {
        legs.push(seller_leg(shop, scope, basket).await);
    }

    (marketplace, basket, legs)
}

/// The same fixture [`ready_with`] builds, under an arbitrary scope and
/// carrying a basket's id on the cart — a normal, single-scope cart in every
/// way except that it is one leg of a shared checkout.
async fn seller_leg(shop: &Shop, scope: Scope, basket_id: OrderBasketId) -> Ready {
    let mut tx = shop.begin_as(scope).await;
    let ctx = Ctx::new(scope, Actor::System, shop.host.as_ref() as &dyn Host);
    seed_currency(&mut tx, scope).await;

    sqlx::query(
        "insert into payment_provider (id, scope, code, is_enabled) values ($1, $2, $3, true)",
    )
    .bind(Uuid::now_v7())
    .bind(scope.0)
    .bind("bank")
    .execute(&mut *tx)
    .await
    .expect("a payment provider");

    let location = inventory::create_stock_location(
        &mut tx,
        &ctx,
        inventory::NewStockLocation {
            name: format!("warehouse {}", Uuid::now_v7()),
            address: None,
        },
    )
    .await
    .expect("a location");

    let item = inventory::create_inventory_item(
        &mut tx,
        &ctx,
        inventory::NewInventoryItem {
            sku: Some(format!("sku-{}", Uuid::now_v7())),
            title: Some("a thing".into()),
            requires_shipping: true,
        },
    )
    .await
    .expect("an inventory item");

    inventory::set_stock(&mut tx, &ctx, item.id, location.id, 10, 0)
        .await
        .expect("a level");

    let product = Uuid::now_v7();
    sqlx::query("insert into product (id, scope, handle, title) values ($1, $2, $3, $4)")
        .bind(product)
        .bind(scope.0)
        .bind(format!("thing-{product}"))
        .bind("A thing")
        .execute(&mut *tx)
        .await
        .expect("a product");

    let variant = VariantId::new();
    sqlx::query(
        "insert into product_variant (id, scope, product_id, title) values ($1, $2, $3, $4)",
    )
    .bind(variant.as_uuid())
    .bind(scope.0)
    .bind(product)
    .bind("The only one")
    .execute(&mut *tx)
    .await
    .expect("a variant");

    inventory::attach_inventory_item(&mut tx, &ctx, variant, item.id, 1)
        .await
        .expect("the variant to consume the item");

    let address = Uuid::now_v7();
    sqlx::query(
        "insert into cart_address (id, scope, address_1, city, country_code)
         values ($1, $2, '1 Example Street', 'Istanbul', 'TR')",
    )
    .bind(address)
    .bind(scope.0)
    .execute(&mut *tx)
    .await
    .expect("an address");

    let cart = CartId::new();
    sqlx::query(
        "insert into cart
             (id, scope, currency_code, email, shipping_address_id, idempotency_key, basket_id)
         values ($1, $2, 'TRY', 'shopper@example.com', $3, $4, $5)",
    )
    .bind(cart.as_uuid())
    .bind(scope.0)
    .bind(address)
    .bind(format!("key-{}", cart.as_uuid()))
    .bind(basket_id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("a cart");

    cart::add_line(
        &mut tx,
        &ctx,
        cart,
        cart::AddLine {
            variant_id: variant,
            quantity: 1,
            unit_price: Money::new(dec!(10), Currency::parse("TRY").expect("a currency")),
            is_tax_inclusive: false,
            selling_plan_id: None,
        },
    )
    .await
    .expect("a line");

    tx.commit().await.expect("to commit the seed");

    Ready {
        cart_id: cart,
        location_id: location.id,
        variant_id: variant,
    }
}

/// The grouping a host is expected to do: for a scope it knows touched a
/// basket, `cart::for_basket` under that scope's own `Ctx` finds exactly that
/// scope's own leg and nothing of any other seller's.
#[tokio::test]
async fn a_basket_s_carts_group_correctly_by_seller() -> Result<()> {
    let shop = Shop::open().await;
    let (_, basket_id, legs) = basket_with_legs(&shop, &[shop.here, shop.elsewhere]).await;

    let mut tx = shop.begin().await;
    let mine = cart::for_basket(&mut tx, &shop.ctx(), basket_id, tezgah::Paging::first(10))
        .await
        .expect("this scope's own leg");
    tx.rollback().await.expect("to give the connection back");
    assert_eq!(mine.items.len(), 1);
    assert_eq!(mine.items[0].id, legs[0].cart_id);

    let mut tx = shop.begin_as(shop.elsewhere).await;
    let theirs = cart::for_basket(
        &mut tx,
        &shop.theirs(),
        basket_id,
        tezgah::Paging::first(10),
    )
    .await
    .expect("the other scope's own leg");
    tx.rollback().await.expect("to give the connection back");
    assert_eq!(theirs.items.len(), 1);
    assert_eq!(theirs.items[0].id, legs[1].cart_id);

    shop.close().await;
    Ok(())
}

/// The shape README.md now describes: a host opens a basket, opens one cart
/// per seller scope carrying it, and places each leg under that scope's own
/// `Ctx`. Two orders land, both under the one basket, and only one payment
/// collection ever exists — the basket's, attached after both legs are in.
#[tokio::test]
async fn a_checkout_splits_across_seller_scopes_and_pays_once() -> Result<()> {
    let shop = Shop::open().await;
    let (marketplace_scope, basket_id, legs) =
        basket_with_legs(&shop, &[shop.here, shop.elsewhere]).await;

    let bank = Bank::saying(Answer::Authorize);
    let ctxs = [shop.ctx(), shop.theirs()];
    let mut order_ids = Vec::new();
    for (leg, ctx) in legs.iter().zip(&ctxs) {
        let checkout = Checkout::new(
            Arc::clone(&bank) as Arc<dyn PaymentProvider>,
            leg.location_id,
        );
        let placed = checkout
            .place(&shop.pool, ctx, leg.cart_id, Some(basket_id))
            .await?;
        assert_eq!(placed.run.state, State::Done);
        order_ids.push(placed.order_id.expect("a basket leg still places an order"));
    }
    assert_eq!(order_ids.len(), 2);
    assert_eq!(
        *bank.authorized.lock(),
        0,
        "a basket leg pays nothing of its own"
    );

    let seller_a_order: (Option<Uuid>, Option<Uuid>) = {
        let mut tx = shop.begin().await;
        let row: (Option<Uuid>, Option<Uuid>) = sqlx::query_as(
            "select basket_id, payment_collection_id from \"order\" where scope = $1 and id = $2",
        )
        .bind(shop.here.0)
        .bind(order_ids[0].as_uuid())
        .fetch_one(&mut *tx)
        .await
        .expect("seller A's own order");
        tx.rollback().await.expect("to give the connection back");
        row
    };
    assert_eq!(seller_a_order.0, Some(basket_id.as_uuid()));
    assert_eq!(
        seller_a_order.1, None,
        "a basket leg's order defers to the basket's own collection"
    );

    // Now the marketplace attaches the one collection every order under this
    // basket pays from.
    let marketplace_ctx = Ctx::new(
        marketplace_scope,
        Actor::System,
        shop.host.as_ref() as &dyn Host,
    );
    let mut tx = shop.begin_as(marketplace_scope).await;
    let collection = payment::create_collection(
        &mut tx,
        &marketplace_ctx,
        payment::NewCollection {
            amount: Money::new(dec!(20), Currency::parse("TRY").expect("a currency")),
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("the basket's own collection");
    order_basket::attach_payment_collection(&mut tx, &marketplace_ctx, basket_id, collection.id)
        .await
        .expect("to attach it to the basket");
    tx.commit().await.expect("to commit");

    // Seller A still cannot read seller B's order through its own connection.
    let mut tx = shop.begin().await;
    let refused = order::get(&mut tx, &shop.ctx(), order_ids[1])
        .await
        .expect_err("seller A's own connection cannot see seller B's order");
    assert!(refused.is_not_found());
    tx.rollback().await.expect("to give the connection back");

    shop.close().await;
    Ok(())
}
