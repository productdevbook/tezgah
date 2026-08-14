//! Every illegal move, tried.
//!
//! Not a sample of the interesting ones: the whole cross product of every
//! status against every other, for each state machine that has one. The table
//! each machine is allowed to walk is written out below rather than derived, so
//! adding a move is a line in a diff somebody reads.
//!
//! Both halves are checked, because either can be skipped. The code refuses
//! with a conflict; the schema refuses with a check constraint, and would still
//! refuse a writer that never went through this crate at all. Where the schema
//! holds less than the code does, the test says so in as many words rather than
//! implying the two agree.

mod common;

use common::Shop;
use rust_decimal_macros::dec;
use tezgah::fulfilment::{self, NewFulfillment, NewFulfillmentItem};
use tezgah::id::{
    CaptureId, FulfillmentId, OrderId, OrderItemId, RefundId, ReturnId, StockLocationId,
};
use tezgah::money::{Currency, Money};
use tezgah::order::{
    self, ChangeType, NewOrder, NewOrderLine, OrderStatus, ReceivedLine, ReturnLine, can_transition,
};
use tezgah::payment::{self, NewCollection, NewSession, SessionStatus};
use tezgah::ports::Ctx;

fn lira() -> Currency {
    Currency::parse("TRY").expect("a currency code")
}

// ---------------------------------------------------------------------------
// The tables
// ---------------------------------------------------------------------------

const STATUSES: [OrderStatus; 6] = [
    OrderStatus::Draft,
    OrderStatus::Pending,
    OrderStatus::RequiresAction,
    OrderStatus::Completed,
    OrderStatus::Canceled,
    OrderStatus::Archived,
];

/// Where an order may go from where it is. Staying put is always allowed — a
/// retried request is a no-op — and is left out of every row here rather than
/// repeated six times.
const ORDER_MOVES: [(OrderStatus, &[OrderStatus]); 6] = [
    (
        OrderStatus::Draft,
        &[OrderStatus::Pending, OrderStatus::Canceled],
    ),
    (
        OrderStatus::Pending,
        &[
            OrderStatus::RequiresAction,
            OrderStatus::Completed,
            OrderStatus::Canceled,
        ],
    ),
    (
        OrderStatus::RequiresAction,
        &[
            OrderStatus::Pending,
            OrderStatus::Completed,
            OrderStatus::Canceled,
        ],
    ),
    (
        OrderStatus::Completed,
        &[OrderStatus::Archived, OrderStatus::Canceled],
    ),
    (OrderStatus::Canceled, &[]),
    (OrderStatus::Archived, &[]),
];

fn is_allowed(from: OrderStatus, to: OrderStatus) -> bool {
    if from == to {
        return true;
    }
    ORDER_MOVES
        .iter()
        .find(|(status, _)| *status == from)
        .map(|(_, moves)| moves.contains(&to))
        .expect("every status has a row in the table")
}

/// Every status the schema will hold, per table. A value outside its list is a
/// constraint violation whichever writer offers it.
const ORDER_STATUSES: [(&str, OrderStatus); 6] = [
    ("draft", OrderStatus::Draft),
    ("pending", OrderStatus::Pending),
    ("requires_action", OrderStatus::RequiresAction),
    ("completed", OrderStatus::Completed),
    ("canceled", OrderStatus::Canceled),
    ("archived", OrderStatus::Archived),
];

const SESSION_STATUSES: [&str; 6] = [
    "pending",
    "requires_more",
    "authorized",
    "captured",
    "canceled",
    "error",
];

/// A session that has been authorised keeps its `authorized_at`, so only these
/// three can carry one — an authorisation is not unwound by rewriting a status.
const SESSION_AFTER_AUTHORISING: [&str; 3] = ["authorized", "captured", "canceled"];

const CHANGE_STATUSES: [&str; 5] = ["pending", "requested", "confirmed", "declined", "canceled"];

/// A change may still be settled from these and from nothing else.
const CHANGE_OPEN: [&str; 2] = ["pending", "requested"];

/// Every status `order_return` will hold.
const RETURN_STATUSES: [&str; 5] = [
    "requested",
    "open",
    "partially_received",
    "received",
    "canceled",
];

/// A return may still take a parcel in from these and from nothing else. There
/// is no `cancel_return`, so `canceled` is only ever reached from outside this
/// crate; it is in the table because the schema holds it.
const RETURN_OPEN: [&str; 3] = ["requested", "open", "partially_received"];

/// A fulfilment's state is its timestamps, so it is named here rather than in
/// a column: nothing has happened, it is packed, it has gone, it arrived, or it
/// was called back before it left.
const FULFILMENT_STATES: [&str; 5] = ["new", "packed", "shipped", "delivered", "canceled"];

const FULFILMENT_MOVES: [(&str, &[&str]); 5] = [
    // Shipping a parcel nobody packed backfills `packed_at`: the box left, so
    // it was packed, whoever forgot to say so.
    ("new", &["pack", "ship", "cancel"]),
    ("packed", &["ship", "cancel"]),
    ("shipped", &["deliver"]),
    ("delivered", &[]),
    ("canceled", &[]),
];

fn fulfilment_allows(from: &str, mv: &str) -> bool {
    FULFILMENT_MOVES
        .iter()
        .find(|(state, _)| *state == from)
        .map(|(_, moves)| moves.contains(&mv))
        .expect("every fulfilment state has a row in the table")
}

/// The values `order.payment_status` and `order.fulfillment_status` will hold.
const ORDER_PAYMENT_STATUSES: [&str; 10] = [
    "not_paid",
    "awaiting",
    "authorized",
    "partially_authorized",
    "partially_captured",
    "captured",
    "partially_refunded",
    "refunded",
    "canceled",
    "requires_action",
];

const ORDER_FULFILMENT_STATUSES: [&str; 10] = [
    "not_fulfilled",
    "partially_fulfilled",
    "fulfilled",
    "partially_shipped",
    "shipped",
    "partially_delivered",
    "delivered",
    "partially_returned",
    "returned",
    "canceled",
];

const NOT_A_STATUS: [&str; 4] = ["shipped", "paid", "", "PENDING"];

/// What Postgres calls a check constraint violation.
const CHECK_VIOLATION: &str = "23514";

// ---------------------------------------------------------------------------
// The order's own machine
// ---------------------------------------------------------------------------

#[test]
fn the_table_of_moves_is_what_the_code_believes() {
    for from in STATUSES {
        for to in STATUSES {
            assert_eq!(
                can_transition(from, to),
                is_allowed(from, to),
                "the table and the code disagree about {} to {}",
                from.as_str(),
                to.as_str()
            );
        }
    }
}

#[test]
fn a_final_status_has_nothing_after_it() {
    for from in STATUSES {
        for to in STATUSES {
            if from.is_final() && from != to {
                assert!(
                    !can_transition(from, to),
                    "{} is final and moved to {}",
                    from.as_str(),
                    to.as_str()
                );
            }
        }
    }
}

async fn an_order_in(shop: &Shop, ctx: &Ctx<'_>, status: OrderStatus) -> OrderId {
    let mut tx = shop.begin().await;
    let mut new = NewOrder::of(lira());
    new.lines.push(NewOrderLine::of(
        "A kettle",
        1,
        Money::new(dec!(20), lira()),
    ));

    let order = match status {
        OrderStatus::Draft => order::create_draft(&mut tx, ctx, new)
            .await
            .expect("a draft"),
        _ => order::create(&mut tx, ctx, new).await.expect("an order"),
    };

    let walk: &[OrderStatus] = match status {
        OrderStatus::Draft | OrderStatus::Pending => &[],
        OrderStatus::RequiresAction => &[OrderStatus::RequiresAction],
        OrderStatus::Completed => &[OrderStatus::Completed],
        OrderStatus::Canceled => &[OrderStatus::Canceled],
        OrderStatus::Archived => &[OrderStatus::Completed, OrderStatus::Archived],
    };
    for step in walk {
        order::set_status(&mut tx, ctx, order.id, *step)
            .await
            .expect("a move the table allows");
    }

    let placed = order::get(&mut tx, ctx, order.id).await.expect("the order");
    assert_eq!(
        placed.status().expect("a status"),
        status,
        "the fixture did not reach {}",
        status.as_str()
    );
    tx.commit().await.expect("to commit");

    order.id
}

#[tokio::test]
async fn every_move_an_order_cannot_make_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    for from in STATUSES {
        for to in STATUSES {
            let order = an_order_in(&shop, &ctx, from).await;

            let mut tx = shop.begin().await;
            let moved = order::set_status(&mut tx, &ctx, order, to).await;

            match moved {
                Ok(order) => {
                    assert!(
                        is_allowed(from, to),
                        "{} moved to {} and the table does not allow it",
                        from.as_str(),
                        to.as_str()
                    );
                    assert_eq!(
                        order.status().expect("a status"),
                        to,
                        "{} to {} returned another status",
                        from.as_str(),
                        to.as_str()
                    );
                    tx.commit().await.expect("to commit");
                }
                Err(err) => {
                    assert!(
                        !is_allowed(from, to),
                        "{} to {} is in the table and was refused as {}",
                        from.as_str(),
                        to.as_str(),
                        err.code()
                    );
                    assert!(
                        err.is_conflict(),
                        "{} to {} was refused as {} rather than a conflict",
                        from.as_str(),
                        to.as_str(),
                        err.code()
                    );
                    tx.rollback().await.expect("to roll back");
                }
            }
        }
    }

    shop.close().await;
}

/// The schema holds the walk as well as the values now, so a legal move from
/// `pending` is taken and an illegal one is refused whoever is writing —
/// including a writer that never went near this crate.
#[tokio::test]
async fn the_schema_takes_the_moves_the_code_would_make_and_no_others() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

    for (status, to) in ORDER_STATUSES {
        let mut tx = shop.begin().await;
        let written = sqlx::query(r#"update "order" set status = $1 where id = $2"#)
            .bind(status)
            .bind(order.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");

        if is_allowed(OrderStatus::Pending, to) {
            assert!(
                written.is_ok(),
                "the schema refused {status}, which pending is allowed to become"
            );
        } else {
            assert!(
                written.is_err(),
                "the schema let an order go from pending to {status} behind the code"
            );
        }
    }

    shop.close().await;
}

/// The schema holds the statuses, not the walk between them.
///
/// A check constraint sees one row and cannot see where it came from, so the
/// one-way walk is the code's alone. What the schema does hold is the pairing
/// with `canceled_at`, and that is what stops a canceled order being quietly
/// reopened by a writer that skipped this crate.
#[tokio::test]
async fn a_canceled_order_cannot_be_reopened_behind_the_code() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Canceled).await;

    for (status, _) in ORDER_STATUSES {
        if status == "canceled" {
            continue;
        }
        let mut tx = shop.begin().await;
        let refused = sqlx::query(r#"update "order" set status = $1 where id = $2"#)
            .bind(status)
            .bind(order.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "a canceled order was moved to {status} by a plain update"
        );
    }

    shop.close().await;
}

// ---------------------------------------------------------------------------
// A payment session
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_payment_session_holds_only_the_statuses_the_code_knows() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let session = {
        let mut tx = shop.begin().await;
        payment::register_provider(&mut tx, &ctx, "fake")
            .await
            .expect("a provider");
        let collection = payment::create_collection(
            &mut tx,
            &ctx,
            NewCollection {
                amount: Money::new(dec!(100.00), lira()),
                cart_id: None,
                metadata: None,
            },
        )
        .await
        .expect("a collection");
        let session = payment::create_session(
            &mut tx,
            &ctx,
            NewSession {
                collection_id: collection.id,
                provider_code: "fake".into(),
                amount: Money::new(dec!(100.00), lira()),
                context: None,
            },
        )
        .await
        .expect("a session");
        tx.commit().await.expect("to commit");
        session.id
    };

    for status in SESSION_STATUSES {
        assert_eq!(
            SessionStatus::parse(status).as_str(),
            status,
            "the code and the schema name {status} differently"
        );

        let mut tx = shop.begin().await;
        let written = sqlx::query("update payment_session set status = $1 where id = $2")
            .bind(status)
            .bind(session.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            written.is_ok(),
            "the schema refused {status}, which the code writes"
        );
    }

    for status in NOT_A_STATUS {
        let mut tx = shop.begin().await;
        let refused = sqlx::query("update payment_session set status = $1 where id = $2")
            .bind(status)
            .bind(session.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "the schema accepted {status:?} as a session's status"
        );
    }

    shop.close().await;
}

#[tokio::test]
async fn an_authorised_session_cannot_be_walked_back_to_an_open_one() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    let session = {
        let mut tx = shop.begin().await;
        payment::register_provider(&mut tx, &ctx, "fake")
            .await
            .expect("a provider");
        let collection = payment::create_collection(
            &mut tx,
            &ctx,
            NewCollection {
                amount: Money::new(dec!(100.00), lira()),
                cart_id: None,
                metadata: None,
            },
        )
        .await
        .expect("a collection");
        let session = payment::create_session(
            &mut tx,
            &ctx,
            NewSession {
                collection_id: collection.id,
                provider_code: "fake".into(),
                amount: Money::new(dec!(100.00), lira()),
                context: None,
            },
        )
        .await
        .expect("a session");
        tx.commit().await.expect("to commit");
        session.id
    };

    for status in SESSION_STATUSES {
        let mut tx = shop.begin().await;
        let written = sqlx::query(
            "update payment_session set status = $1, authorized_at = now() where id = $2",
        )
        .bind(status)
        .bind(session.as_uuid())
        .execute(&mut *tx)
        .await;
        tx.rollback().await.expect("to roll back");

        if SESSION_AFTER_AUTHORISING.contains(&status) {
            assert!(
                written.is_ok(),
                "the schema refused an authorised session in {status}"
            );
        } else {
            assert!(
                is_a_check_violation(&written),
                "an authorised session was walked back to {status}"
            );
        }
    }

    shop.close().await;
}

// ---------------------------------------------------------------------------
// A change to an order
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_change_can_only_be_settled_out_of_the_states_it_is_open_in() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    for from in CHANGE_STATUSES {
        for settle in ["confirm", "decline"] {
            let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

            let mut tx = shop.begin().await;
            let change = order::request_change(&mut tx, &ctx, order, ChangeType::Edit, None)
                .await
                .expect("a change");
            // The two coupling constraints want the timestamp beside the status.
            sqlx::query(
                "update order_change
                 set status = $1,
                     confirmed_at = case when $1 = 'confirmed' then now() end,
                     declined_at = case when $1 = 'declined' then now() end
                 where id = $2",
            )
            .bind(from)
            .bind(change.id.as_uuid())
            .execute(&mut *tx)
            .await
            .expect("to put the change in that state");
            tx.commit().await.expect("to commit");

            let mut tx = shop.begin().await;
            let settled = match settle {
                "confirm" => order::confirm_change(&mut tx, &ctx, change.id)
                    .await
                    .map(|_| ()),
                _ => order::decline_change(&mut tx, &ctx, change.id, None)
                    .await
                    .map(|_| ()),
            };

            match settled {
                Ok(()) => {
                    assert!(
                        CHANGE_OPEN.contains(&from),
                        "a change in {from} was {settle}ed"
                    );
                    tx.commit().await.expect("to commit");
                }
                Err(err) => {
                    assert!(
                        !CHANGE_OPEN.contains(&from),
                        "a change in {from} could not be {settle}ed: {}",
                        err.code()
                    );
                    assert!(
                        err.is_conflict(),
                        "settling a change in {from} was refused as {}",
                        err.code()
                    );
                    tx.rollback().await.expect("to roll back");
                }
            }
        }
    }

    shop.close().await;
}

#[tokio::test]
async fn a_change_holds_only_the_statuses_the_code_knows() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

    let change = {
        let mut tx = shop.begin().await;
        let change = order::request_change(&mut tx, &ctx, order, ChangeType::Edit, None)
            .await
            .expect("a change");
        tx.commit().await.expect("to commit");
        change.id
    };

    for status in CHANGE_STATUSES {
        let mut tx = shop.begin().await;
        let written = sqlx::query(
            "update order_change
             set status = $1,
                 confirmed_at = case when $1 = 'confirmed' then now() end,
                 declined_at = case when $1 = 'declined' then now() end
             where id = $2",
        )
        .bind(status)
        .bind(change.as_uuid())
        .execute(&mut *tx)
        .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            written.is_ok(),
            "the schema refused {status}, which the code writes"
        );
    }

    for status in NOT_A_STATUS {
        let mut tx = shop.begin().await;
        let refused = sqlx::query("update order_change set status = $1 where id = $2")
            .bind(status)
            .bind(change.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "the schema accepted {status:?} as a change's status"
        );
    }

    // A confirmed change with no timestamp beside it is the same lie told the
    // other way round, and is refused too.
    for (status, column) in [("confirmed", "confirmed_at"), ("declined", "declined_at")] {
        let mut tx = shop.begin().await;
        let refused = sqlx::query(&format!(
            "update order_change set status = $1, {column} = null where id = $2"
        ))
        .bind(status)
        .bind(change.as_uuid())
        .execute(&mut *tx)
        .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "a change was {status} with no {column}"
        );
    }

    shop.close().await;
}

/// Whether Postgres refused this with a check constraint rather than anything
/// else — a foreign key or a lost connection would prove nothing here.
fn is_a_check_violation<T>(outcome: &Result<T, sqlx::Error>) -> bool {
    outcome
        .as_ref()
        .err()
        .and_then(|err| err.as_database_error())
        .and_then(|err| err.code())
        .is_some_and(|code| code == CHECK_VIOLATION)
}

// ---------------------------------------------------------------------------
// A return
// ---------------------------------------------------------------------------

/// An order of two kettles, all of them asked back, and the return forced into
/// `status` afterwards. `canceled_at` is written beside `canceled` because the
/// code reads the timestamp rather than the word.
async fn a_return_in(shop: &Shop, ctx: &Ctx<'_>, status: &str) -> (ReturnId, OrderId) {
    let mut tx = shop.begin().await;
    let mut new = NewOrder::of(lira());
    new.lines.push(NewOrderLine::of(
        "A kettle",
        2,
        Money::new(dec!(20), lira()),
    ));
    let order = order::create(&mut tx, ctx, new).await.expect("an order");
    let line = order::line_items(&mut tx, ctx, order.id)
        .await
        .expect("its lines")
        .first()
        .expect("a line")
        .id;

    let asked = order::request_return(
        &mut tx,
        ctx,
        order.id,
        None,
        vec![ReturnLine {
            order_line_item_id: line,
            quantity: 2,
            return_reason_id: None,
            note: None,
        }],
    )
    .await
    .expect("a return");

    sqlx::query(
        "update order_return
         set status = $1,
             received_at = case when $1 in ('received', 'partially_received') then now() end,
             canceled_at = case when $1 = 'canceled' then now() end
         where id = $2",
    )
    .bind(status)
    .bind(asked.id.as_uuid())
    .execute(&mut *tx)
    .await
    .expect("to put the return in that state");
    tx.commit().await.expect("to commit");

    (asked.id, order.id)
}

async fn a_returned_line(shop: &Shop, ctx: &Ctx<'_>, order: OrderId) -> tezgah::id::LineItemId {
    let mut tx = shop.begin().await;
    let line = order::line_items(&mut tx, ctx, order)
        .await
        .expect("its lines")
        .first()
        .expect("a line")
        .id;
    tx.commit().await.expect("to commit");
    line
}

#[tokio::test]
async fn a_return_can_only_be_received_out_of_the_states_it_is_open_in() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    for from in RETURN_STATUSES {
        let (asked, order) = a_return_in(&shop, &ctx, from).await;
        let line = a_returned_line(&shop, &ctx, order).await;

        let mut tx = shop.begin().await;
        let received = order::receive_return(
            &mut tx,
            &ctx,
            asked,
            vec![ReceivedLine {
                order_line_item_id: line,
                quantity: 1,
                damaged: 0,
            }],
        )
        .await;

        match received {
            Ok(_) => {
                assert!(
                    RETURN_OPEN.contains(&from),
                    "a return in {from} took a parcel in"
                );
                tx.commit().await.expect("to commit");
            }
            Err(err) => {
                assert!(
                    !RETURN_OPEN.contains(&from),
                    "a return in {from} could not take a parcel in: {}",
                    err.code()
                );
                assert!(
                    err.is_conflict(),
                    "receiving into a return in {from} was refused as {}",
                    err.code()
                );
                tx.rollback().await.expect("to roll back");
            }
        }
    }

    shop.close().await;
}

/// The one that decides whether stock goes back twice.
#[tokio::test]
async fn a_return_that_has_been_received_cannot_be_received_again() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (asked, order) = a_return_in(&shop, &ctx, "requested").await;
    let line = a_returned_line(&shop, &ctx, order).await;

    let mut tx = shop.begin().await;
    let settled = order::receive_return(
        &mut tx,
        &ctx,
        asked,
        vec![ReceivedLine {
            order_line_item_id: line,
            quantity: 2,
            damaged: 0,
        }],
    )
    .await
    .expect("the whole parcel");
    assert_eq!(settled.status, "received");
    tx.commit().await.expect("to commit");

    let mut tx = shop.begin().await;
    let again = order::receive_return(
        &mut tx,
        &ctx,
        asked,
        vec![ReceivedLine {
            order_line_item_id: line,
            quantity: 1,
            damaged: 0,
        }],
    )
    .await;
    tx.rollback().await.expect("to roll back");

    assert!(
        again.expect_err("a second receipt").is_conflict(),
        "a received return was received a second time"
    );

    shop.close().await;
}

#[tokio::test]
async fn a_return_holds_only_the_statuses_the_code_knows() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (asked, _) = a_return_in(&shop, &ctx, "requested").await;

    for status in RETURN_STATUSES {
        let mut tx = shop.begin().await;
        let written = sqlx::query(
            "update order_return
             set status = $1, canceled_at = case when $1 = 'canceled' then now() end
             where id = $2",
        )
        .bind(status)
        .bind(asked.as_uuid())
        .execute(&mut *tx)
        .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            written.is_ok(),
            "the schema refused {status}, which the code writes"
        );
    }

    for status in NOT_A_STATUS {
        let mut tx = shop.begin().await;
        let refused = sqlx::query("update order_return set status = $1 where id = $2")
            .bind(status)
            .bind(asked.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "the schema accepted {status:?} as a return's status"
        );
    }

    shop.close().await;
}

/// Receiving a return puts stock back for goods somebody sent, so a return that
/// says `canceled` must not be receivable however it came to say it. Both
/// halves hold now: the schema will not let the word be written without the
/// moment, and the code reads the word as well as the moment.
#[tokio::test]
async fn a_return_cancelled_in_word_only_cannot_be_received() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (asked, order) = a_return_in(&shop, &ctx, "requested").await;
    let line = a_returned_line(&shop, &ctx, order).await;

    let mut tx = shop.begin().await;
    let written = sqlx::query(
        "update order_return set status = 'canceled', canceled_at = null where id = $1",
    )
    .bind(asked.as_uuid())
    .execute(&mut *tx)
    .await;
    tx.rollback().await.expect("to roll back");
    assert!(
        is_a_check_violation(&written),
        "the schema took a cancelled return with no moment of cancelling"
    );

    let mut tx = shop.begin().await;
    sqlx::query("update order_return set status = 'canceled', canceled_at = now() where id = $1")
        .bind(asked.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("to cancel the return");

    let received = order::receive_return(
        &mut tx,
        &ctx,
        asked,
        vec![ReceivedLine {
            order_line_item_id: line,
            quantity: 1,
            damaged: 0,
        }],
    )
    .await;
    let dismissed = order::dismiss_return(
        &mut tx,
        &ctx,
        asked,
        vec![ReceivedLine {
            order_line_item_id: line,
            quantity: 1,
            damaged: 0,
        }],
    )
    .await;
    tx.rollback().await.expect("to roll back");

    assert!(
        received.expect_err("a cancelled return").is_conflict(),
        "a cancelled return took a parcel in"
    );
    assert!(
        dismissed.expect_err("a cancelled return").is_conflict(),
        "a cancelled return dismissed a line"
    );

    shop.close().await;
}

// ---------------------------------------------------------------------------
// A fulfilment, whose machine is its timestamps
// ---------------------------------------------------------------------------

// A state this test does not know is a typo in the table above, not a case.
#[expect(clippy::panic, reason = "a typo in the table must stop the test")]
async fn a_fulfilment_in(
    shop: &Shop,
    ctx: &Ctx<'_>,
    state: &str,
) -> (OrderId, FulfillmentId, OrderItemId) {
    let order = an_order_in(shop, ctx, OrderStatus::Pending).await;

    let mut tx = shop.begin().await;
    let item = order::items(&mut tx, ctx, order, 1)
        .await
        .expect("its items")
        .first()
        .expect("an item")
        .id;

    let location = StockLocationId::new();
    sqlx::query("insert into stock_location (id, scope, name) values ($1, $2, $3)")
        .bind(location.as_uuid())
        .bind(shop.here.0)
        .bind(format!("Depot {location}"))
        .execute(&mut *tx)
        .await
        .expect("a location");

    let made = fulfilment::create_fulfillment(
        &mut tx,
        ctx,
        order,
        NewFulfillment {
            location_id: location,
            shipping_option_id: None,
            provider_id: None,
            requires_shipping: true,
            created_by: None,
            data: None,
            address: None,
            items: vec![NewFulfillmentItem {
                order_item_id: item,
                inventory_item_id: None,
                title: "A kettle".into(),
                sku: None,
                barcode: None,
                quantity: 1,
            }],
        },
    )
    .await
    .expect("a fulfilment");

    let walk: &[&str] = match state {
        "new" => &[],
        "packed" => &["pack"],
        "shipped" => &["pack", "ship"],
        "delivered" => &["pack", "ship", "deliver"],
        "canceled" => &["cancel"],
        _ => panic!("{state} is not a fulfilment state"),
    };
    for step in walk {
        make_the_move(&mut tx, ctx, order, made.id, step)
            .await
            .expect("a move the table allows");
    }
    tx.commit().await.expect("to commit");

    (order, made.id, item)
}

// A state this test does not know is a typo in the table above, not a case.
#[expect(clippy::panic, reason = "a typo in the table must stop the test")]
async fn make_the_move(
    tx: &mut tezgah::ports::Tx<'_>,
    ctx: &Ctx<'_>,
    order: OrderId,
    id: FulfillmentId,
    mv: &str,
) -> tezgah::Result<()> {
    match mv {
        "pack" => fulfilment::mark_packed(tx, ctx, order, id)
            .await
            .map(|_| ()),
        "ship" => fulfilment::mark_shipped(tx, ctx, order, id, None)
            .await
            .map(|_| ()),
        "deliver" => fulfilment::mark_delivered(tx, ctx, order, id)
            .await
            .map(|_| ()),
        "cancel" => fulfilment::cancel_fulfillment(tx, ctx, order, id)
            .await
            .map(|_| ()),
        _ => panic!("{mv} is not a move"),
    }
}

#[tokio::test]
async fn every_move_a_fulfilment_cannot_make_is_refused() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();

    for from in FULFILMENT_STATES {
        for mv in ["pack", "ship", "deliver", "cancel"] {
            let (order, made, _) = a_fulfilment_in(&shop, &ctx, from).await;

            let mut tx = shop.begin().await;
            let moved = make_the_move(&mut tx, &ctx, order, made, mv).await;

            match moved {
                Ok(()) => {
                    assert!(
                        fulfilment_allows(from, mv),
                        "a fulfilment in {from} was {mv}ed and the table does not allow it"
                    );
                    tx.commit().await.expect("to commit");
                }
                Err(err) => {
                    assert!(
                        !fulfilment_allows(from, mv),
                        "a fulfilment in {from} could not be {mv}ed: {}",
                        err.code()
                    );
                    assert!(
                        err.is_conflict(),
                        "{mv} on a fulfilment in {from} was refused as {}",
                        err.code()
                    );
                    tx.rollback().await.expect("to roll back");
                }
            }
        }
    }

    shop.close().await;
}

/// The order the timestamps may be written in is the schema's too, so a writer
/// that never went through this crate cannot deliver a parcel that never
/// shipped or call back one that has.
#[tokio::test]
async fn the_schema_holds_the_order_the_timestamps_are_written_in() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (_, made, _) = a_fulfilment_in(&shop, &ctx, "new").await;

    for (columns, refused) in [
        ("shipped_at = now()", true),
        ("delivered_at = now()", true),
        ("packed_at = now()", false),
        ("packed_at = now(), shipped_at = now()", false),
        (
            "packed_at = now(), shipped_at = now(), canceled_at = now()",
            true,
        ),
        (
            "packed_at = now(), shipped_at = now(), delivered_at = now()",
            false,
        ),
    ] {
        let mut tx = shop.begin().await;
        let written = sqlx::query(&format!("update fulfillment set {columns} where id = $1"))
            .bind(made.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");

        if refused {
            assert!(
                is_a_check_violation(&written),
                "the schema accepted {columns} on a fulfilment"
            );
        } else {
            assert!(written.is_ok(), "the schema refused {columns}");
        }
    }

    shop.close().await;
}

// ---------------------------------------------------------------------------
// The two statuses an operator reads
// ---------------------------------------------------------------------------
//
// `order.payment_status` and `order.fulfillment_status` are not state machines:
// there is no walk from one to another, only the answer the ledger and the item
// counters give. Database triggers write them and nothing else does, so what is
// proven below is the schema's list of words, and that each column says what
// the rows it is computed from say.

const NOT_ONE_OF_THOSE: [&str; 4] = ["nope", "", "NOT_PAID", "part_paid"];

#[tokio::test]
async fn an_order_holds_only_the_payment_and_fulfilment_statuses_the_schema_lists() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

    for (column, statuses) in [
        ("payment_status", ORDER_PAYMENT_STATUSES),
        ("fulfillment_status", ORDER_FULFILMENT_STATUSES),
    ] {
        for status in statuses {
            let mut tx = shop.begin().await;
            let written = sqlx::query(&format!(
                r#"update "order" set {column} = $1 where id = $2"#
            ))
            .bind(status)
            .bind(order.as_uuid())
            .execute(&mut *tx)
            .await;
            tx.rollback().await.expect("to roll back");
            assert!(written.is_ok(), "the schema refused {column} = {status}");
        }

        for status in NOT_ONE_OF_THOSE {
            let mut tx = shop.begin().await;
            let refused = sqlx::query(&format!(
                r#"update "order" set {column} = $1 where id = $2"#
            ))
            .bind(status)
            .bind(order.as_uuid())
            .execute(&mut *tx)
            .await;
            tx.rollback().await.expect("to roll back");
            assert!(
                is_a_check_violation(&refused),
                "the schema accepted {status:?} as {column}"
            );
        }
    }

    shop.close().await;
}

/// A parcel goes out and the order says so. The column is written by a trigger
/// off `order_item`'s counters, so it moves whoever moved the goods.
#[tokio::test]
async fn fulfillment_status_moves_when_the_parcel_does() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (order, made, _) = a_fulfilment_in(&shop, &ctx, "new").await;

    let mut tx = shop.begin().await;
    let packed = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(packed.fulfillment_status, "fulfilled");
    assert_eq!(packed.payment_status, "not_paid");

    fulfilment::mark_shipped(&mut tx, &ctx, order, made, None)
        .await
        .expect("the parcel to leave");
    let gone = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(gone.fulfillment_status, "shipped");

    fulfilment::mark_delivered(&mut tx, &ctx, order, made)
        .await
        .expect("the parcel to arrive");
    let arrived = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(arrived.fulfillment_status, "delivered");
    tx.rollback().await.expect("to roll back");

    shop.close().await;
}

/// And falls back when the fulfilment is called off: the counters go down and
/// the column is only ever their answer.
#[tokio::test]
async fn fulfillment_status_falls_back_when_the_fulfilment_is_cancelled() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (order, made, _) = a_fulfilment_in(&shop, &ctx, "new").await;

    let mut tx = shop.begin().await;
    fulfilment::cancel_fulfillment(&mut tx, &ctx, order, made)
        .await
        .expect("the fulfilment to be called off");
    let seen = order::get(&mut tx, &ctx, order).await.expect("the order");
    tx.rollback().await.expect("to roll back");

    assert_eq!(
        seen.fulfillment_status, "not_fulfilled",
        "the order still claims goods went out"
    );

    shop.close().await;
}

/// `payment_status` is the ledger's answer, written by a trigger off
/// `order_transaction`. The two are asserted equal rather than separately:
/// a column that could disagree with `ledger` is the whole bug.
#[tokio::test]
async fn payment_status_follows_the_ledger() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

    let mut tx = shop.begin().await;
    let placed = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(placed.payment_status, "not_paid");

    order::record_transaction(
        &mut tx,
        &ctx,
        order,
        Money::new(dec!(20), lira()),
        "payment",
        CaptureId::new().as_uuid(),
    )
    .await
    .expect("the hold");
    let held = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(held.payment_status, "authorized");

    order::record_transaction(
        &mut tx,
        &ctx,
        order,
        Money::new(dec!(8), lira()),
        "capture",
        CaptureId::new().as_uuid(),
    )
    .await
    .expect("part of the money");
    let part = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(part.payment_status, "partially_captured");

    order::record_transaction(
        &mut tx,
        &ctx,
        order,
        Money::new(dec!(12), lira()),
        "capture",
        CaptureId::new().as_uuid(),
    )
    .await
    .expect("the rest of the money");
    let whole = order::get(&mut tx, &ctx, order).await.expect("the order");
    assert_eq!(whole.payment_status, "captured");

    order::record_transaction(
        &mut tx,
        &ctx,
        order,
        Money::new(dec!(-5), lira()),
        "refund",
        RefundId::new().as_uuid(),
    )
    .await
    .expect("some of it back");
    let back = order::get(&mut tx, &ctx, order).await.expect("the order");
    let counted = order::ledger(&mut tx, &ctx, order)
        .await
        .expect("the ledger");
    tx.rollback().await.expect("to roll back");

    assert_eq!(back.payment_status, "partially_refunded");
    assert_eq!(
        back.payment_status,
        counted.state.as_str(),
        "the column and the ledger disagree"
    );

    shop.close().await;
}

/// Neither column is anybody else's to read.
#[tokio::test]
async fn another_scope_sees_neither_status() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let (order, made, _) = a_fulfilment_in(&shop, &ctx, "new").await;

    let mut tx = shop.begin().await;
    fulfilment::mark_shipped(&mut tx, &ctx, order, made, None)
        .await
        .expect("the parcel to leave");
    tx.commit().await.expect("to commit");

    let theirs = shop.theirs();
    let mut tx = shop.begin_as(shop.elsewhere).await;
    let looked = order::get(&mut tx, &theirs, order).await;
    let counted: i64 = sqlx::query_scalar(
        r#"select count(*) from "order" where fulfillment_status <> 'not_fulfilled'"#,
    )
    .fetch_one(&mut *tx)
    .await
    .expect("a count");
    tx.rollback().await.expect("to roll back");

    assert!(
        looked.expect_err("somebody else's order").is_not_found(),
        "another scope read the order"
    );
    assert_eq!(counted, 0, "another scope saw a shipped order");

    shop.close().await;
}
