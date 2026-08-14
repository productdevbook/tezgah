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
use tezgah::id::OrderId;
use tezgah::money::{Currency, Money};
use tezgah::order::{self, ChangeType, NewOrder, NewOrderLine, OrderStatus, can_transition};
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
const ORDER_STATUSES: [&str; 6] = [
    "draft",
    "pending",
    "requires_action",
    "completed",
    "canceled",
    "archived",
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

#[tokio::test]
async fn a_status_nothing_writes_is_refused_by_the_schema_as_well() {
    let shop = Shop::open().await;
    let ctx = shop.ctx();
    let order = an_order_in(&shop, &ctx, OrderStatus::Pending).await;

    for status in ORDER_STATUSES {
        let mut tx = shop.begin().await;
        let written = sqlx::query(r#"update "order" set status = $1 where id = $2"#)
            .bind(status)
            .bind(order.as_uuid())
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
        let refused = sqlx::query(r#"update "order" set status = $1 where id = $2"#)
            .bind(status)
            .bind(order.as_uuid())
            .execute(&mut *tx)
            .await;
        tx.rollback().await.expect("to roll back");
        assert!(
            is_a_check_violation(&refused),
            "the schema accepted {status:?} as an order's status"
        );
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

    for status in ORDER_STATUSES {
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
