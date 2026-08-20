//! Who is shopping, and how they prove it.
//!
//! The same seam as `identity`, on the other side of the shop. tezgah
//! authenticates nobody, so a customer's password is the host's — and until
//! there was one, this binary produced `Actor::Guest` for every storefront
//! request with the cart id read out of the URL. `http::store`'s own doc
//! comment said what that cost: four declared routes left unbound because
//! every one of them calls `signed_in` first and would have answered `denied`
//! to every caller it could ever have.
//!
//! A shopper is a `customer` row with `has_account`, which is tezgah's, plus
//! a credential, which is this binary's. The two are joined by
//! `server_shopper.customer_id`, and the credential table carries no `scope`
//! for the same reason `server_operator` does not: a password is not one of
//! the shop's rows.
//!
//! Registration is deliberately not idempotent on e-mail. A second
//! registration with an address somebody already holds is refused rather than
//! folded into the first — an account that quietly becomes somebody else's is
//! the failure this is guarding.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::identity;

/// The same thirty days an operator's session lasts, and not extended by use.
const SESSION_DAYS: i64 = 30;

#[derive(Debug)]
pub struct IssuedSession {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub customer_id: Uuid,
}

pub async fn create_tables(pool: &PgPool) -> tezgah::Result<()> {
    sqlx::query(
        "create table if not exists server_shopper (
             customer_id uuid primary key,
             email text not null,
             password_hash text not null,
             created_at timestamptz not null default now(),
             disabled_at timestamptz
         )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "create unique index if not exists server_shopper_email
         on server_shopper (lower(email))",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "create table if not exists server_shopper_session (
             id uuid primary key,
             customer_id uuid not null references server_shopper (customer_id) on delete cascade,
             token_digest bytea not null unique,
             created_at timestamptz not null default now(),
             expires_at timestamptz not null,
             last_seen_at timestamptz
         )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "create index if not exists server_shopper_session_customer
         on server_shopper_session (customer_id)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// The credential half of registering. The `customer` row is tezgah's and is
/// written by the caller, in its own transaction, before this is reached — so
/// a refusal here leaves an account with no password rather than a password
/// with no account, and the caller says which.
pub async fn attach_credential(
    pool: &PgPool,
    customer_id: Uuid,
    email: &str,
    password: &str,
) -> tezgah::Result<()> {
    if password.chars().count() < 12 {
        return Err(tezgah::Error::invalid(
            "a password is at least twelve characters",
        ));
    }

    let hash = identity::hash_password(password)?;

    sqlx::query(
        "insert into server_shopper (customer_id, email, password_hash)
         values ($1, $2, $3)",
    )
    .bind(customer_id)
    .bind(email)
    .bind(&hash)
    .execute(pool)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(ref db) if db.code().as_deref() == Some("23505") => {
            tezgah::Error::conflict("somebody already shops with that e-mail address")
        }
        other => tezgah::Error::from(other),
    })?;

    Ok(())
}

/// Whether an address is already spoken for, asked before the `customer` row
/// is written so a refused registration leaves nothing behind.
pub async fn taken(pool: &PgPool, email: &str) -> tezgah::Result<bool> {
    let (count,): (i64,) =
        sqlx::query_as("select count(*) from server_shopper where lower(email) = lower($1)")
            .bind(email)
            .fetch_one(pool)
            .await?;
    Ok(count > 0)
}

/// The same answer to an address nobody holds and a password that is wrong,
/// and the same argon2 hash run either way — an address that is not
/// registered must not be the faster refusal.
pub async fn sign_in(pool: &PgPool, email: &str, password: &str) -> tezgah::Result<IssuedSession> {
    let found: Option<(Uuid, String, Option<DateTime<Utc>>)> = sqlx::query_as(
        "select customer_id, password_hash, disabled_at
         from server_shopper
         where lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    let Some((customer_id, hash, disabled_at)) = found else {
        let _ = identity::hash_password(password);
        return Err(tezgah::Error::denied());
    };

    if disabled_at.is_some() || !identity::password_matches(password, &hash) {
        return Err(tezgah::Error::denied());
    }

    let token = identity::mint_token();
    let expires_at = Utc::now() + Duration::days(SESSION_DAYS);

    sqlx::query(
        "insert into server_shopper_session (id, customer_id, token_digest, expires_at)
         values ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(customer_id)
    .bind(identity::digest(&token))
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(IssuedSession {
        token,
        expires_at,
        customer_id,
    })
}

/// Which customer is holding this token, if anybody still is.
pub async fn session_customer(pool: &PgPool, token: &str) -> tezgah::Result<Option<Uuid>> {
    let found: Option<(Uuid,)> = sqlx::query_as(
        "update server_shopper_session s
         set last_seen_at = now()
         from server_shopper c
         where s.token_digest = $1
           and s.expires_at > now()
           and c.customer_id = s.customer_id
           and c.disabled_at is null
         returning c.customer_id",
    )
    .bind(identity::digest(token))
    .fetch_optional(pool)
    .await?;

    Ok(found.map(|(id,)| id))
}

pub async fn sign_out(pool: &PgPool, token: &str) -> tezgah::Result<()> {
    sqlx::query("delete from server_shopper_session where token_digest = $1")
        .bind(identity::digest(token))
        .execute(pool)
        .await?;
    Ok(())
}

/// Swept beside the operators' own, by `schedule`.
pub async fn drop_expired_sessions(pool: &PgPool) -> tezgah::Result<u64> {
    let gone = sqlx::query("delete from server_shopper_session where expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(gone.rows_affected())
}
