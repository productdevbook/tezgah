//! Sending an outbox row somewhere.
//!
//! `EventSink` writes `server_event` in the transaction of the change that
//! caused it, which is what makes an event a thing that happened rather than
//! a thing somebody hoped happened. Delivering it is the other half, and
//! until this module existed there was none: `delivered_at` was null on
//! every row, for ever, and a shop wanting `order.paid` anywhere else polled
//! the table.
//!
//! One destination, one signature, one worker. That is deliberately less
//! than a subscription system: this binary is one shop, and a shop that needs
//! events fanned out to five places puts something that does that behind the
//! one URL. What the product owes is that the event leaves the building, once
//! per event, with something the receiver can check.
//!
//! **At least once, never exactly once.** A row is marked delivered after the
//! receiver answered, so a crash between the answer and the update sends it
//! again. The receiver is told the event's id and is expected to have seen it
//! before — the same contract every payment provider states, and the same one
//! `payment::record_webhook` implements on the way in.

use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::Sha256;
use sqlx::PgPool;
use uuid::Uuid;

/// How many times a refused event is tried before it is left alone.
///
/// The same number the job worker uses, for the same reason: five doublings
/// from a minute reach about half an hour, which covers a receiver that was
/// restarting and not one that has been switched off.
const MAX_ATTEMPTS: i32 = 5;

/// How many are taken per tick. Small, because each is a request over the
/// network and a slow receiver should not hold a claim on fifty rows.
const BATCH: i64 = 10;

#[derive(Clone)]
pub struct Destination {
    pub url: Arc<str>,
    pub secret: Arc<str>,
}

/// Written out rather than derived, so the secret cannot reach a log through
/// a `{:?}` somebody added to an error message later.
impl std::fmt::Debug for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Destination")
            .field("url", &self.url)
            .field("secret", &"…")
            .finish()
    }
}

/// The columns the table needs to retry, added to whatever is already there.
///
/// `alter table ... if not exists` rather than a second `create`: an
/// installation running before this module existed has rows in `server_event`
/// and should keep them.
pub async fn prepare(pool: &PgPool) -> tezgah::Result<()> {
    for column in [
        "attempts integer not null default 0",
        "failure text",
        "dead_at timestamptz",
        "next_attempt_at timestamptz",
    ] {
        sqlx::query(&format!(
            "alter table server_event add column if not exists {column}"
        ))
        .execute(pool)
        .await?;
    }

    sqlx::query(
        "create index if not exists server_event_due
         on server_event (next_attempt_at)
         where delivered_at is null and dead_at is null",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub fn spawn(pool: PgPool, destination: Destination) {
    tokio::spawn(async move {
        // Built once. A client per request opens a new connection pool and a
        // new TLS session for every event.
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(client) => client,
            Err(err) => {
                eprintln!("event deliverer: no http client, events stay undelivered: {err}");
                return;
            }
        };

        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(5));
        loop {
            ticks.tick().await;
            if let Err(err) = drain_once(&pool, &client, &destination).await {
                eprintln!("event deliverer: {err}");
            }
        }
    });
}

/// Claims what is due, sends it, and writes down what happened.
///
/// The claim and the outcome are two transactions, the same way the job
/// worker does it: a row's next attempt is pushed out before the request is
/// made, so a second worker will not take the same row while this one waits
/// on a network that may not answer.
pub async fn drain_once(
    pool: &PgPool,
    client: &reqwest::Client,
    destination: &Destination,
) -> tezgah::Result<()> {
    type Row = (Uuid, String, Uuid, serde_json::Value, i32);

    let due: Vec<Row> = {
        let mut tx = pool.begin().await?;

        let claimed: Vec<Row> = sqlx::query_as(
            "select id, name, entity_id, payload, attempts from server_event
             where delivered_at is null
               and dead_at is null
               and (next_attempt_at is null or next_attempt_at <= now())
             order by created_at
             limit $1
             for update skip locked",
        )
        .bind(BATCH)
        .fetch_all(&mut *tx)
        .await?;

        for (id, _, _, _, attempts) in &claimed {
            sqlx::query("update server_event set next_attempt_at = now() + $2 where id = $1")
                .bind(id)
                .bind(backoff(*attempts))
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;
        claimed
    };

    for (id, name, entity_id, payload, attempts) in due {
        let body = serde_json::json!({
            "id": id,
            "name": name,
            "entity_id": entity_id,
            "payload": payload,
        });
        let text = serde_json::to_string(&body).map_err(|err| {
            tezgah::Error::invalid(format!("an event that will not serialise: {err}"))
        })?;

        match send(client, destination, &text).await {
            Ok(()) => {
                sqlx::query(
                    "update server_event
                     set delivered_at = now(), failure = null, next_attempt_at = null
                     where id = $1",
                )
                .bind(id)
                .execute(pool)
                .await?;
            }
            Err(reason) => {
                let spent = attempts + 1 >= MAX_ATTEMPTS;
                sqlx::query(
                    "update server_event
                     set attempts = attempts + 1,
                         failure = $2,
                         dead_at = case when $3 then now() else dead_at end
                     where id = $1",
                )
                .bind(id)
                .bind(&reason)
                .bind(spent)
                .execute(pool)
                .await?;

                // Said out loud, because a dead event is one the shop has to
                // do something about by hand — the row keeps the reason.
                if spent {
                    eprintln!("event {id} ({name}) gave up after {MAX_ATTEMPTS}: {reason}");
                }
            }
        }
    }

    Ok(())
}

async fn send(
    client: &reqwest::Client,
    destination: &Destination,
    body: &str,
) -> Result<(), String> {
    let response = client
        .post(destination.url.as_ref())
        .header("content-type", "application/json")
        .header("tezgah-signature", signature(&destination.secret, body))
        .body(body.to_owned())
        .send()
        .await
        .map_err(|err| format!("no answer: {err}"))?;

    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("answered {}", response.status().as_u16()))
    }
}

/// `sha256=<hex>` over the exact bytes sent, which is the convention a
/// receiver already has code for.
///
/// The signature is over the body rather than over the fields, because the
/// receiver checks what arrived rather than what it parsed — a body that
/// re-serialises differently is the classic way a valid signature stops
/// matching.
pub fn signature(secret: &str, body: &str) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("hmac takes a key of any length");
    mac.update(body.as_bytes());
    let digest = mac.finalize().into_bytes();

    let mut out = String::from("sha256=");
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Doubling from a minute, the same shape the job worker retries with.
fn backoff(attempts: i32) -> sqlx::postgres::types::PgInterval {
    let seconds = 60_i64 * i64::from(1 << attempts.clamp(0, 5));
    sqlx::postgres::types::PgInterval {
        months: 0,
        days: 0,
        microseconds: seconds * 1_000_000,
    }
}
