//! Gift cards and store credit, from the route to the ledger.
//!
//! The module had tests and no route, so `RedeemCredit` returned from its first
//! `if` on every checkout that ever ran. What is asserted here is the whole
//! chain rather than any one link: issue a card through the admin route, put it
//! on a cart through the storefront route, check out, and find
//! `order_credit_line` written, the collection's `credit_amount` moved and the
//! card spent.

mod common;

use std::sync::Arc;

use common::{OnlyMine, Shop, Teller};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::api::credit as route;
use tezgah::checkout::Checkout;
use tezgah::id::CustomerId;
use tezgah::payment::{self, PaymentProvider};
use tezgah::ports::{Actor, Ctx, Host};
use tezgah::workflow::State;
use uuid::Uuid;

fn lira(amount: Decimal) -> route::AmountIn {
    route::AmountIn {
        amount,
        currency_code: "TRY".into(),
    }
}

async fn credit_lines(shop: &Shop, order: Uuid) -> (i64, Decimal) {
    let mut tx = shop.begin().await;
    let row: (i64, Option<Decimal>) = sqlx::query_as(
        "select count(*), sum(amount) from order_credit_line where scope = $1 and order_id = $2",
    )
    .bind(shop.here.0)
    .bind(order)
    .fetch_one(&mut *tx)
    .await
    .expect("the credit lines");
    tx.rollback().await.expect("to roll back");

    (row.0, row.1.unwrap_or(Decimal::ZERO))
}

#[tokio::test]
async fn a_gift_card_issued_on_a_route_is_spent_by_a_checkout() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let issued = route::issue_gift_card(
        &mut tx,
        &ctx,
        route::IssueGiftCard {
            balance: lira(dec!(15)),
            customer_id: None,
            issued_order_id: None,
            expires_at: None,
            reason: Some("a test".into()),
        },
    )
    .await
    .expect("a gift card");

    assert_eq!(issued.card.balance, dec!(15));
    assert!(!issued.code.is_empty());

    // The lookup answers by hash and never hands the code back.
    let found = route::find_gift_card(
        &mut tx,
        &ctx,
        route::GiftCardCode {
            code: issued.code.clone(),
        },
    )
    .await
    .expect("the card the code names");
    assert_eq!(found.id, issued.card.id);

    let applied = route::apply_gift_card(
        &mut tx,
        &ctx,
        here.cart_id,
        route::ApplyGiftCard {
            code: issued.code.clone(),
            amount: lira(dec!(15)),
        },
    )
    .await
    .expect("the cart to take the card");
    assert_eq!(applied.gift_card_id, Some(issued.card.id));

    let on_the_cart = route::list_cart_credits(&mut tx, &ctx, here.cart_id)
        .await
        .expect("what the cart will pay with");
    assert_eq!(on_the_cart.len(), 1);

    tx.commit().await.expect("to commit");

    let checkout = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id)
        .await
        .expect("a checkout");
    assert_eq!(
        placed.run.state,
        State::Done,
        "the checkout unwound: {:?}",
        placed.run.failure
    );

    let order_id = placed.order_id.expect("an order");
    let (lines, total) = credit_lines(&shop, order_id.as_uuid()).await;
    assert_eq!(lines, 1, "nothing was written to order_credit_line");
    assert_eq!(total, dec!(15));

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();

    let card = route::get_gift_card(&mut tx, &ctx, issued.card.id)
        .await
        .expect("the card");
    assert_eq!(card.balance, Decimal::ZERO, "the card was not spent");

    let moved = route::gift_card_movements(&mut tx, &ctx, issued.card.id, route::List::default())
        .await
        .expect("the ledger");
    assert_eq!(moved.items.len(), 2, "an issue and a redemption");

    let order = tezgah::order::get(&mut tx, &ctx, order_id)
        .await
        .expect("the order");
    let collection = payment::collection(
        &mut tx,
        &ctx,
        order.payment_collection_id.expect("a collection"),
    )
    .await
    .expect("the collection");
    assert_eq!(
        collection.credit_amount,
        dec!(15),
        "the collection did not count the card"
    );

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

/// Two carts, one card, at the same moment on two connections. The card is
/// worth fifteen and both carts want fifteen: whatever the interleaving, the
/// shop hands over fifteen lira of goods and not thirty.
#[tokio::test]
async fn one_card_spent_twice_at_once_comes_off_once() {
    let shop = Shop::open().await;
    let first = common::a_cart_ready(&shop, 10, 2).await;
    let second = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let issued = route::issue_gift_card(
        &mut tx,
        &ctx,
        route::IssueGiftCard {
            balance: lira(dec!(15)),
            customer_id: None,
            issued_order_id: None,
            expires_at: None,
            reason: None,
        },
    )
    .await
    .expect("a gift card");

    for cart in [first.cart_id, second.cart_id] {
        route::apply_gift_card(
            &mut tx,
            &ctx,
            cart,
            route::ApplyGiftCard {
                code: issued.code.clone(),
                amount: lira(dec!(15)),
            },
        )
        .await
        .expect("the cart to take the card");
    }
    tx.commit().await.expect("to commit");

    let one = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        first.location_id,
    );
    let two = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        second.location_id,
    );

    let ctx = shop.ctx();
    let (left, right) = tokio::join!(
        one.place(&shop.pool, &ctx, first.cart_id),
        two.place(&shop.pool, &ctx, second.cart_id),
    );

    let mut spent = Decimal::ZERO;
    for placed in [left.expect("a checkout"), right.expect("a checkout")] {
        let order = placed.order_id.expect("an order");
        let (_, amount) = credit_lines(&shop, order.as_uuid()).await;
        spent += amount;
    }
    assert_eq!(spent, dec!(15), "the card paid for more than it was worth");

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let card = route::get_gift_card(&mut tx, &ctx, issued.card.id)
        .await
        .expect("the card");
    assert_eq!(card.balance, Decimal::ZERO);
    tx.rollback().await.expect("to roll back");

    shop.close().await;
}

#[tokio::test]
async fn store_credit_granted_on_a_route_is_readable_and_spendable() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let customer = common::a_customer(&mut tx, &ctx).await;

    sqlx::query("update cart set customer_id = $3 where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(here.cart_id.as_uuid())
        .bind(customer.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the cart to belong to somebody");

    let account = route::adjust_store_credit(
        &mut tx,
        &ctx,
        customer,
        route::AdjustStoreCredit {
            amount: lira(dec!(20)),
            reason: Some("goodwill".into()),
        },
    )
    .await
    .expect("a balance");
    assert_eq!(account.balance, dec!(20));

    let read = route::get_store_credit(
        &mut tx,
        &ctx,
        customer,
        route::BalanceQuery {
            currency_code: "TRY".into(),
        },
    )
    .await
    .expect("the balance");
    assert_eq!(read.balance, dec!(20));

    let moved = route::store_credit_movements(&mut tx, &ctx, account.id, route::List::default())
        .await
        .expect("the ledger");
    assert_eq!(moved.items.len(), 1);
    tx.commit().await.expect("to commit");

    // As the shopper themselves, against their own cart.
    let host = OnlyMine {
        customer: customer.as_uuid(),
    };
    let mine = Ctx::new(
        shop.here,
        Actor::Customer {
            id: customer.as_uuid(),
        },
        &host as &dyn Host,
    );

    let mut tx = shop.begin().await;
    route::apply_store_credit(
        &mut tx,
        &mine,
        here.cart_id,
        route::ApplyStoreCredit {
            amount: lira(dec!(20)),
        },
    )
    .await
    .expect("the cart to take the balance");
    tx.commit().await.expect("to commit");

    let checkout = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id)
        .await
        .expect("a checkout");

    let order_id = placed.order_id.expect("an order");
    let (lines, total) = credit_lines(&shop, order_id.as_uuid()).await;
    assert_eq!(lines, 1);
    assert_eq!(total, dec!(20), "the whole order was paid from the balance");

    shop.close().await;
}

#[tokio::test]
async fn another_customers_cart_is_not_mine_to_pay_from() {
    let shop = Shop::open().await;
    let here = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let owner = common::a_customer(&mut tx, &ctx).await;

    sqlx::query("update cart set customer_id = $3 where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(here.cart_id.as_uuid())
        .bind(owner.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("the cart to belong to somebody");

    let issued = route::issue_gift_card(
        &mut tx,
        &ctx,
        route::IssueGiftCard {
            balance: lira(dec!(15)),
            customer_id: None,
            issued_order_id: None,
            expires_at: None,
            reason: None,
        },
    )
    .await
    .expect("a gift card");
    tx.commit().await.expect("to commit");

    let stranger = CustomerId::new();
    let host = OnlyMine {
        customer: stranger.as_uuid(),
    };
    let theirs = Ctx::new(
        shop.here,
        Actor::Customer {
            id: stranger.as_uuid(),
        },
        &host as &dyn Host,
    );

    let mut tx = shop.begin().await;
    let refused = route::apply_gift_card(
        &mut tx,
        &theirs,
        here.cart_id,
        route::ApplyGiftCard {
            code: issued.code.clone(),
            amount: lira(dec!(15)),
        },
    )
    .await
    .expect_err("somebody else's cart");
    assert!(refused.is_denied());

    let refused = route::list_cart_credits(&mut tx, &theirs, here.cart_id)
        .await
        .expect_err("somebody else's cart");
    assert!(refused.is_denied());

    let refused = route::apply_store_credit(
        &mut tx,
        &theirs,
        here.cart_id,
        route::ApplyStoreCredit {
            amount: lira(dec!(5)),
        },
    )
    .await
    .expect_err("somebody else's cart");
    assert!(refused.is_denied());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_credit_taken_off_a_cart_is_not_charged() {
    let shop = Shop::open().await;
    let here: common::Shelf = common::a_cart_ready(&shop, 10, 2).await;

    let mut tx = shop.begin().await;
    let ctx = shop.ctx();
    let issued = route::issue_gift_card(
        &mut tx,
        &ctx,
        route::IssueGiftCard {
            balance: lira(dec!(15)),
            customer_id: None,
            issued_order_id: None,
            expires_at: None,
            reason: None,
        },
    )
    .await
    .expect("a gift card");

    let applied = route::apply_gift_card(
        &mut tx,
        &ctx,
        here.cart_id,
        route::ApplyGiftCard {
            code: issued.code,
            amount: lira(dec!(15)),
        },
    )
    .await
    .expect("the cart to take the card");

    route::remove_cart_credit(&mut tx, &ctx, here.cart_id, applied.id)
        .await
        .expect("to take it off again");

    let left: Vec<route::CartCreditView> = route::list_cart_credits(&mut tx, &ctx, here.cart_id)
        .await
        .expect("what is left");
    assert!(left.is_empty());
    tx.commit().await.expect("to commit");

    let checkout = Checkout::new(
        Arc::new(Teller) as Arc<dyn PaymentProvider>,
        here.location_id,
    );
    let placed = checkout
        .place(&shop.pool, &shop.ctx(), here.cart_id)
        .await
        .expect("a checkout");
    let order_id = placed.order_id.expect("an order");

    let (lines, _) = credit_lines(&shop, order_id.as_uuid()).await;
    assert_eq!(lines, 0);

    shop.close().await;
}
