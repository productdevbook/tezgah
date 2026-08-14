//! Gift cards and store credit, against a real Postgres.
//!
//! The ones that matter: two people spending the last of one card at the same
//! moment, and a balance that always equals the sum of its ledger. Everything
//! else here is a rule somebody will otherwise assume rather than check — an
//! expiry, a wrong code, a second scope reaching across.

mod common;

use common::Shop;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tezgah::credit::{self, NewGiftCard, Redemption};
use tezgah::id::{GiftCardId, OrderId, PaymentCollectionId};
use tezgah::money::{Currency, Money};
use tezgah::order::{self, NewOrder, NewOrderLine, NewTaxLine, TaxSnapshot};
use tezgah::page::Paging;
use tezgah::payment::{self, NewCollection};
use tezgah::ports::{Ctx, Tx};

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

fn money(amount: Decimal) -> Money {
    Money::new(amount, lira())
}

async fn a_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, balance: Decimal) -> (GiftCardId, String) {
    let issued = credit::issue(
        tx,
        ctx,
        NewGiftCard {
            balance: money(balance),
            issued_order_id: None,
            customer_id: None,
            expires_at: None,
            reason: None,
        },
    )
    .await
    .expect("a gift card");

    (issued.card.id, issued.code)
}

/// An order with one line, so there is something for a refund to leave.
async fn an_order(tx: &mut Tx<'_>, ctx: &Ctx<'_>, total: Decimal) -> OrderId {
    let customer = common::a_customer(tx, ctx).await;

    order::create(
        tx,
        ctx,
        NewOrder {
            customer_id: Some(customer),
            lines: vec![NewOrderLine::of("A thing", 1, money(total))],
            ..NewOrder::of(lira())
        },
    )
    .await
    .expect("an order")
    .id
}

async fn a_collection(tx: &mut Tx<'_>, ctx: &Ctx<'_>, total: Decimal) -> PaymentCollectionId {
    payment::create_collection(
        tx,
        ctx,
        NewCollection {
            amount: money(total),
            cart_id: None,
            metadata: None,
        },
    )
    .await
    .expect("a payment collection")
    .id
}

/// The sum of a card's ledger, which the balance column has to equal.
async fn ledger_sum(tx: &mut Tx<'_>, scope: uuid::Uuid, card: GiftCardId) -> Decimal {
    sqlx::query_scalar(
        "select coalesce(sum(amount), 0) from gift_card_transaction
         where scope = $1 and gift_card_id = $2",
    )
    .bind(scope)
    .bind(card.as_uuid())
    .fetch_one(&mut **tx)
    .await
    .expect("the ledger")
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_balance_is_the_sum_of_its_ledger() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let (card, _) = a_card(&mut tx, &ctx, dec!(100)).await;

    for amount in [dec!(10), dec!(25.50), dec!(4.50)] {
        credit::redeem_gift_card(&mut tx, &ctx, card, money(amount), &Redemption::default())
            .await
            .expect("a redemption");
    }

    let after = credit::gift_card(&mut tx, &ctx, card)
        .await
        .expect("the card");
    let sum = ledger_sum(&mut tx, shop.here.0, card).await;

    assert_eq!(after.balance, dec!(60.00));
    assert_eq!(after.balance, sum, "the balance and its ledger disagree");

    let ledger = credit::gift_card_ledger(&mut tx, &ctx, card, Paging::first(50))
        .await
        .expect("the ledger");
    assert_eq!(ledger.len(), 4, "an issue and three redemptions");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn two_people_spending_the_last_of_one_card_and_one_of_them_getting_it() {
    let shop = Shop::open().await;

    let mut setup = shop.begin().await;
    let (card, _) = a_card(&mut setup, &shop.ctx(), dec!(50)).await;
    setup.commit().await.expect("to keep the card");

    let spend = || async {
        let mut tx = shop.begin().await;
        let taken = credit::redeem_gift_card(
            &mut tx,
            &shop.ctx(),
            card,
            money(dec!(50)),
            &Redemption::default(),
        )
        .await;

        match taken {
            Ok(_) => {
                tx.commit().await.expect("to keep the redemption");
                Ok(())
            }
            Err(err) => {
                tx.rollback().await.expect("to give it back");
                Err(err)
            }
        }
    };

    // Two connections at the same moment, not one after the other.
    let (first, second) = tokio::join!(spend(), spend());

    assert_eq!(
        i32::from(first.is_ok()) + i32::from(second.is_ok()),
        1,
        "fifty lira went to both of them or to neither"
    );

    let mut after = shop.begin().await;
    let left = credit::gift_card(&mut after, &shop.ctx(), card)
        .await
        .expect("the card");
    let sum = ledger_sum(&mut after, shop.here.0, card).await;
    after.commit().await.expect("to finish reading");

    assert_eq!(left.balance, Decimal::ZERO, "a card went negative");
    assert_eq!(left.balance, sum);

    shop.close().await;
}

#[tokio::test]
async fn a_card_past_its_expiry_will_not_be_spent() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let issued = credit::issue(
        &mut tx,
        &ctx,
        NewGiftCard {
            balance: money(dec!(100)),
            issued_order_id: None,
            customer_id: None,
            expires_at: Some(ctx.now() + chrono::Duration::days(1)),
            reason: None,
        },
    )
    .await
    .expect("a gift card");

    // Moved into the past behind the code's back, which is the only way to get
    // an expired card without waiting a day.
    sqlx::query("update gift_card set expires_at = $3 where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(issued.card.id.as_uuid())
        .bind(ctx.now() - chrono::Duration::days(1))
        .execute(&mut *tx)
        .await
        .expect("to age the card");

    let refused = credit::redeem_gift_card(
        &mut tx,
        &ctx,
        issued.card.id,
        money(dec!(10)),
        &Redemption::default(),
    )
    .await
    .expect_err("an expired card is not spendable");
    assert!(refused.is_conflict());

    let still = credit::gift_card(&mut tx, &ctx, issued.card.id)
        .await
        .expect("the card");
    assert_eq!(still.balance, dec!(100));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_disabled_card_will_not_be_spent() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let (card, _) = a_card(&mut tx, &ctx, dec!(100)).await;

    credit::disable_gift_card(&mut tx, &ctx, card)
        .await
        .expect("to disable it");

    let refused =
        credit::redeem_gift_card(&mut tx, &ctx, card, money(dec!(10)), &Redemption::default())
            .await
            .expect_err("a disabled card is not spendable");
    assert!(refused.is_conflict());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_code_is_handed_back_once_and_is_never_stored() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let (card, code) = a_card(&mut tx, &ctx, dec!(100)).await;

    let found = credit::gift_card_by_code(&mut tx, &ctx, &code)
        .await
        .expect("the code finds its card");
    assert_eq!(found.id, card);

    // The code itself is nowhere in the row: only its hash was kept, so a
    // leaked table is not a pile of spendable cards.
    let plaintext: i64 =
        sqlx::query_scalar("select count(*) from gift_card where scope = $1 and code_hash = $2")
            .bind(shop.here.0)
            .bind(&code)
            .fetch_one(&mut *tx)
            .await
            .expect("to look for the code");
    assert_eq!(plaintext, 0, "the code was stored as itself");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_code_that_was_never_issued_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let (_, code) = a_card(&mut tx, &ctx, dec!(100)).await;

    let mut wrong = code.clone();
    wrong.pop();
    wrong.push(if code.ends_with('A') { 'B' } else { 'A' });

    let missing = credit::gift_card_by_code(&mut tx, &ctx, &wrong)
        .await
        .expect_err("a code nobody issued");
    assert!(missing.is_not_found());

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn another_scope_can_neither_see_a_card_nor_spend_it() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let (card, code) = a_card(&mut mine, &shop.ctx(), dec!(100)).await;
    mine.commit().await.expect("to keep the card");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let ctx = shop.theirs();

    assert!(
        credit::gift_card(&mut theirs, &ctx, card)
            .await
            .expect_err("somebody else's card")
            .is_not_found()
    );
    assert!(
        credit::gift_card_by_code(&mut theirs, &ctx, &code)
            .await
            .expect_err("somebody else's code")
            .is_not_found()
    );
    assert!(
        credit::redeem_gift_card(
            &mut theirs,
            &ctx,
            card,
            money(dec!(1)),
            &Redemption::default()
        )
        .await
        .is_err()
    );
    theirs.rollback().await.expect("to roll back");

    let mut after = shop.begin().await;
    let left = credit::gift_card(&mut after, &shop.ctx(), card)
        .await
        .expect("the card");
    after.commit().await.expect("to finish reading");
    assert_eq!(left.balance, dec!(100));

    shop.close().await;
}

#[tokio::test]
async fn a_card_that_does_not_cover_the_basket_leaves_the_rest_for_a_provider() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let collection = a_collection(&mut tx, &ctx, dec!(400)).await;
    let (card, _) = a_card(&mut tx, &ctx, dec!(150)).await;

    credit::redeem_gift_card(
        &mut tx,
        &ctx,
        card,
        money(dec!(150)),
        &Redemption {
            payment_collection_id: Some(collection),
            ..Redemption::default()
        },
    )
    .await
    .expect("a redemption");

    let after = payment::collection(&mut tx, &ctx, collection)
        .await
        .expect("the collection");

    assert_eq!(after.credit_amount, dec!(150));
    assert_eq!(after.amount, dec!(400), "the basket still comes to 400");
    assert_eq!(after.due(), dec!(250), "the card is asked for the rest");
    assert_eq!(after.status, "partially_captured");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_card_that_covers_the_basket_leaves_nothing_for_a_provider() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let collection = a_collection(&mut tx, &ctx, dec!(400)).await;
    let (card, _) = a_card(&mut tx, &ctx, dec!(400)).await;

    credit::redeem_gift_card(
        &mut tx,
        &ctx,
        card,
        money(dec!(400)),
        &Redemption {
            payment_collection_id: Some(collection),
            ..Redemption::default()
        },
    )
    .await
    .expect("a redemption");

    let after = payment::collection(&mut tx, &ctx, collection)
        .await
        .expect("the collection");

    assert_eq!(after.due(), Decimal::ZERO);
    assert_eq!(after.status, "captured");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_restored_redemption_nets_the_collection_back_to_nothing() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let collection = a_collection(&mut tx, &ctx, dec!(400)).await;
    let order = an_order(&mut tx, &ctx, dec!(400)).await;
    let (card, _) = a_card(&mut tx, &ctx, dec!(150)).await;

    let what = Redemption {
        order_id: Some(order),
        payment_collection_id: Some(collection),
        reason: None,
    };

    credit::redeem_gift_card(&mut tx, &ctx, card, money(dec!(150)), &what)
        .await
        .expect("a redemption");

    // Twice, the way a workflow runner may replay a compensation.
    credit::restore_gift_card(&mut tx, &ctx, card, money(dec!(150)), &what)
        .await
        .expect("to put it back");
    credit::restore_gift_card(&mut tx, &ctx, card, money(dec!(150)), &what)
        .await
        .expect("to put it back again, harmlessly");

    let after = credit::gift_card(&mut tx, &ctx, card)
        .await
        .expect("the card");
    let sum = ledger_sum(&mut tx, shop.here.0, card).await;
    assert_eq!(after.balance, dec!(150), "a compensation paid twice");
    assert_eq!(after.balance, sum);

    let collection = payment::collection(&mut tx, &ctx, collection)
        .await
        .expect("the collection");
    assert_eq!(collection.credit_amount, Decimal::ZERO);
    assert_eq!(collection.due(), dec!(400));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_refund_can_stay_in_the_shop() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let order = an_order(&mut tx, &ctx, dec!(400)).await;

    let account = credit::refund_to_credit(
        &mut tx,
        &ctx,
        order,
        money(dec!(400)),
        Some("returned, kept as credit".into()),
    )
    .await
    .expect("a refund to credit");

    assert_eq!(account.balance, dec!(400));

    let ledger = credit::store_credit_ledger(&mut tx, &ctx, account.id, Paging::first(50))
        .await
        .expect("the ledger");
    let sum: Decimal = ledger.items.iter().map(|row| row.amount).sum();
    assert_eq!(sum, account.balance, "the balance and its ledger disagree");

    // The money left the order, and no provider was asked for it.
    let out: Decimal = sqlx::query_scalar(
        "select coalesce(sum(amount), 0) from order_transaction
         where scope = $1 and order_id = $2 and reference = 'refund'",
    )
    .bind(shop.here.0)
    .bind(order.as_uuid())
    .fetch_one(&mut *tx)
    .await
    .expect("the order ledger");
    assert_eq!(out, dec!(-400));

    let refunds: i64 = sqlx::query_scalar("select count(*) from refund where scope = $1")
        .bind(shop.here.0)
        .fetch_one(&mut *tx)
        .await
        .expect("the refund table");
    assert_eq!(refunds, 0, "a provider was asked for money it never had");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_balance_cannot_be_spent_past_nothing() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let customer = common::a_customer(&mut tx, &ctx).await;
    let account = credit::grant_store_credit(&mut tx, &ctx, customer, money(dec!(60)), None)
        .await
        .expect("a grant");

    credit::redeem_store_credit(
        &mut tx,
        &ctx,
        account.id,
        money(dec!(60)),
        &Redemption::default(),
    )
    .await
    .expect("the whole of it");

    let refused = credit::redeem_store_credit(
        &mut tx,
        &ctx,
        account.id,
        money(dec!(1)),
        &Redemption::default(),
    )
    .await
    .expect_err("an empty balance");
    assert!(refused.is_conflict());

    let after = credit::store_credit(&mut tx, &ctx, customer, lira())
        .await
        .expect("the balance");
    assert_eq!(after.balance, Decimal::ZERO);

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_gift_card_line_cannot_carry_tax() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;

    let refused = order::create(
        &mut tx,
        &ctx,
        NewOrder {
            lines: vec![NewOrderLine {
                is_giftcard: true,
                tax_lines: vec![NewTaxLine {
                    rate: dec!(20),
                    code: "VAT".into(),
                    name: "VAT".into(),
                    provider_id: None,
                    description: None,
                    snapshot: TaxSnapshot::default(),
                }],
                ..NewOrderLine::of("A gift card", 1, money(dec!(100)))
            }],
            ..NewOrder::of(lira())
        },
    )
    .await
    .expect_err("tax on a gift card is charged twice");
    assert_eq!(refused.code(), "invalid");

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}

#[tokio::test]
async fn a_cart_says_what_it_will_pay_with_before_anything_moves() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let mut tx = shop.begin().await;
    let cart = tezgah::id::CartId::new();
    sqlx::query("insert into cart (id, scope, currency_code) values ($1, $2, 'TRY')")
        .bind(cart.as_uuid())
        .bind(shop.here.0)
        .execute(&mut *tx)
        .await
        .expect("a cart");

    let (card, code) = a_card(&mut tx, &ctx, dec!(100)).await;

    let intent = credit::apply_gift_card(&mut tx, &ctx, cart, &code, money(dec!(40)))
        .await
        .expect("an intent");
    assert_eq!(intent.gift_card_id, Some(card));

    // Saying it twice is saying it once: the cart carries one row per card.
    credit::apply_gift_card(&mut tx, &ctx, cart, &code, money(dec!(60)))
        .await
        .expect("to change its mind");

    let intents = credit::cart_credits(&mut tx, &ctx, cart)
        .await
        .expect("what the cart will pay with");
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].amount, dec!(60));

    // Nothing moved.
    let still = credit::gift_card(&mut tx, &ctx, card)
        .await
        .expect("the card");
    assert_eq!(still.balance, dec!(100));

    tx.rollback().await.expect("to roll back");
    shop.close().await;
}
