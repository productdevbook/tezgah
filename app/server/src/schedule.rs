//! The work nothing asks for.
//!
//! `tests/reachable.rs` in the crate root tolerates `cart::expire` and
//! `inventory::expire_reservations` with the same reason — "a sweep a host
//! runs on a schedule; there is no request to hang it off". This binary is
//! that host, and until now it ran neither: on the shipped image an abandoned
//! cart was never cleared and the stock it had reserved was held for ever.
//!
//! Not a job. `ports::Jobs` is enqueue-only by design — tezgah writes a job
//! in the transaction the change belongs to and never decides when it runs —
//! so recurrence is the host's, and this is where the host keeps it.

use std::time::Duration;

use sqlx::PgPool;
use tezgah::ports::{Actor, Ctx, Host, Scope, Tx};

use crate::host::ServerHost;
use crate::identity;

/// Every five minutes. A reservation's own expiry decides when stock is
/// actually free, not this — the sweep only has to notice, and noticing five
/// minutes late costs a shop nothing a shorter tick would win back.
const EVERY: Duration = Duration::from_secs(300);

/// At most this many carts a pass. `cart::expire` takes at most `MAX_BATCH` a
/// call and says a full answer means there is more; the loop keeps asking
/// until it does not, and this stops one very stale shop from holding a
/// transaction open all afternoon.
const PASSES: usize = 20;

pub fn spawn(pool: PgPool, scope: Scope) {
    tokio::spawn(async move {
        let host = ServerHost;
        let mut ticks = tokio::time::interval(EVERY);
        loop {
            ticks.tick().await;
            if let Err(err) = sweep(&pool, scope, &host).await {
                eprintln!("scheduler: {err}");
            }
        }
    });
}

async fn sweep(pool: &PgPool, scope: Scope, host: &ServerHost) -> tezgah::Result<()> {
    // `Actor::System`: nobody asked for this, and `docs/hosting.md` is
    // explicit that an authorizer denying `Actor::System` silently stops every
    // scheduled thing a shop has.
    let ctx = Ctx::new(scope, Actor::System, host as &dyn Host);

    let mut carts = 0usize;
    for _ in 0..PASSES {
        let mut tx = begin(pool, scope).await?;
        let gone = tezgah::cart::expire(&mut tx, &ctx, ctx.now()).await?;
        tx.commit().await?;

        carts += gone.len();
        if gone.is_empty() {
            break;
        }
    }

    let mut tx = begin(pool, scope).await?;
    let freed = tezgah::inventory::expire_reservations(&mut tx, &ctx, ctx.now()).await?;
    tx.commit().await?;

    let sessions = identity::drop_expired_sessions(pool).await?
        + crate::shopper::drop_expired_sessions(pool).await?;

    if carts > 0 || freed > 0 || sessions > 0 {
        println!(
            "{}",
            serde_json::json!({
                "kind": "swept",
                "carts_expired": carts,
                "reservations_freed": freed,
                "sessions_dropped": sessions,
            })
        );
    }

    Ok(())
}

/// The same two lines `http::begin` writes, and for the same reason:
/// `tezgah::ports::scoped` is `pub(crate)`, so a host announces its own scope
/// on the transaction by hand.
async fn begin(pool: &PgPool, scope: Scope) -> tezgah::Result<Tx<'static>> {
    let mut tx = pool.begin().await?;
    sqlx::query("select set_config('app.scope', $1, true)")
        .bind(scope.0.to_string())
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
