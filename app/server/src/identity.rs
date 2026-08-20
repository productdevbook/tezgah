//! Who is in the back office, and how they prove it.
//!
//! tezgah authenticates nobody on purpose — `docs/hosting.md` and
//! `tezgah::ports::Authorizer` both say so, and a library that invented
//! accounts would hand every host a second set to reconcile. That is right
//! for the library and leaves a hole in the product: `ADMIN_TOKEN` is one
//! shared secret, so a shop with two employees has one credential between
//! them, nothing to revoke when one leaves, and no audit row that can say who
//! changed a price. `http::admin`'s own doc comment named the seam — "a
//! deployment that wants real operators with real identities replaces this
//! middleware" — and this is that deployment.
//!
//! These tables are this binary's, not tezgah's. They carry no `scope` and no
//! row-level security, for the same reason `server_job` does not: a person who
//! runs the shop is not one of the shop's rows.
//!
//! `ADMIN_TOKEN` does not go away. It is the way in before the first operator
//! exists, and the way back in when the last password is lost — so it stays
//! accepted, and `main.rs` says so at startup rather than leaving it to be
//! discovered.

use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use tezgah::ports::Action;
use uuid::Uuid;

/// How long a session lasts from the moment it was issued. Not extended by
/// use: a sliding window means a stolen token stays alive as long as it is
/// being used, which is exactly the case it should not survive.
const SESSION_DAYS: i64 = 30;

/// What `select` hands back for an account, before it is anything else.
/// Named because clippy is right that five anonymous fields is not a type.
type OperatorTuple = (
    Uuid,
    String,
    String,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

/// The same, with the password hash in place of the created-at: what sign-in
/// needs and a listing must never carry.
type CredentialTuple = (Uuid, String, String, String, String, Option<DateTime<Utc>>);

/// What an operator is allowed to do, in three steps.
///
/// The split is the crate's own rather than one invented here.
/// `tezgah::ports::Action` already separates `Settle` — capture, refund,
/// cancel — from `Write`, and says why: "editing an order and refunding one
/// are not one power". So does `Moderate`, for approving somebody else's
/// listing. A role here is which of those five a person may ask for.
///
/// Coarser than an `Authorizer`, and deliberately so: this is enforced at the
/// door, against the `Action` the route table already declares, not against
/// the row a handler is about to read. A shop that needs "this operator may
/// refund orders under a hundred lira" needs an authorizer, which is the port
/// tezgah asks for and this binary answers permissively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Everything, and the only role that may make or disable an account.
    Owner,
    /// The shop's day-to-day: reading, writing, deleting, moderating. Not
    /// moving money.
    Staff,
    /// Reading, and nothing else.
    Viewer,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Owner => "owner",
            Role::Staff => "staff",
            Role::Viewer => "viewer",
        }
    }

    /// An unrecognised value reads as the narrowest role rather than the
    /// widest: a column somebody hand-edited into nonsense should close the
    /// door, not open it.
    pub fn parse(text: &str) -> Role {
        match text {
            "owner" => Role::Owner,
            "staff" => Role::Staff,
            _ => Role::Viewer,
        }
    }

    pub fn may(self, action: Action) -> bool {
        match self {
            Role::Owner => true,
            Role::Staff => !matches!(action, Action::Settle),
            Role::Viewer => matches!(action, Action::View),
        }
    }

    /// Making, renaming and disabling accounts. Not one of tezgah's actions —
    /// these are this binary's own routes — so it is asked separately.
    pub fn may_manage_operators(self) -> bool {
        matches!(self, Role::Owner)
    }
}

/// Somebody who may reach the back office.
#[derive(Debug, Clone)]
pub struct Operator {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
}

/// The one time a session's own token exists in plain text.
#[derive(Debug)]
pub struct IssuedSession {
    pub token: String,
    pub expires_at: DateTime<Utc>,
    pub operator: Operator,
}

#[derive(Debug, Clone)]
pub struct OperatorRow {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    pub disabled_at: Option<DateTime<Utc>>,
}

/// Made ahead of `tezgah::MIGRATIONS`, like `host::create_jobs_table`, and for
/// the same reason: tezgah owns no table by either name and never will.
pub async fn create_tables(pool: &PgPool) -> tezgah::Result<()> {
    sqlx::query(
        "create table if not exists server_operator (
             id uuid primary key,
             email text not null,
             name text not null,
             password_hash text not null,
             created_at timestamptz not null default now(),
             disabled_at timestamptz
         )",
    )
    .execute(pool)
    .await?;

    // The first installation of this table had no roles; an installation with
    // accounts in it keeps them, and they become `staff` rather than `owner` —
    // widening somebody's power in a migration is not a migration's decision.
    sqlx::query(
        "alter table server_operator add column if not exists role text not null default 'staff'",
    )
    .execute(pool)
    .await?;

    // Case-insensitively unique: an address is one address however it was
    // typed, and two accounts differing only in case is a way in, not a
    // feature.
    sqlx::query(
        "create unique index if not exists server_operator_email
         on server_operator (lower(email))",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "create table if not exists server_session (
             id uuid primary key,
             operator_id uuid not null references server_operator (id) on delete cascade,
             token_digest bytea not null unique,
             created_at timestamptz not null default now(),
             expires_at timestamptz not null,
             last_seen_at timestamptz
         )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "create index if not exists server_session_operator
         on server_session (operator_id)",
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn count(pool: &PgPool) -> tezgah::Result<i64> {
    let (total,): (i64,) = sqlx::query_as("select count(*) from server_operator")
        .fetch_one(pool)
        .await?;
    Ok(total)
}

/// Argon2id with its default parameters — the crate's own recommendation, and
/// not a number this file is in a position to tune better.
fn hash_password(password: &str) -> tezgah::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| tezgah::Error::invalid("that password could not be stored"))
}

fn password_matches(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Two v4 uuids rather than a `rand` dependency: v4 is `getrandom`, so this is
/// 244 bits out of the operating system's own generator, which is the only
/// property a session token needs.
fn mint_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// What is stored. A session table full of usable tokens is a second password
/// file, so the row holds a digest and the token itself leaves once.
fn digest(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub async fn create_operator(
    pool: &PgPool,
    email: &str,
    name: &str,
    password: &str,
    role: Role,
) -> tezgah::Result<Operator> {
    if password.chars().count() < 12 {
        return Err(tezgah::Error::invalid(
            "a password is at least twelve characters",
        ));
    }
    if !email.contains('@') {
        return Err(tezgah::Error::invalid("that is not an e-mail address"));
    }

    let id = Uuid::now_v7();
    let hash = hash_password(password)?;

    // The first account is always the owner, whatever was asked for. A shop
    // whose only account cannot make a second one has locked itself out with
    // the key inside — and the only way back would be the `ADMIN_TOKEN` it
    // was told it could stop keeping.
    let role = if count(pool).await? == 0 {
        Role::Owner
    } else {
        role
    };

    sqlx::query(
        "insert into server_operator (id, email, name, password_hash, role)
         values ($1, $2, $3, $4, $5)",
    )
    .bind(id)
    .bind(email)
    .bind(name)
    .bind(&hash)
    .bind(role.as_str())
    .execute(pool)
    .await
    .map_err(|err| match err {
        sqlx::Error::Database(ref db) if db.code().as_deref() == Some("23505") => {
            tezgah::Error::conflict("somebody already has that e-mail address")
        }
        other => tezgah::Error::from(other),
    })?;

    Ok(Operator {
        id,
        email: email.to_owned(),
        name: name.to_owned(),
        role,
    })
}

pub async fn list_operators(pool: &PgPool) -> tezgah::Result<Vec<OperatorRow>> {
    let rows: Vec<OperatorTuple> = sqlx::query_as(
        "select id, email, name, role, created_at, disabled_at
         from server_operator
         order by created_at",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, email, name, role, created_at, disabled_at)| OperatorRow {
                id,
                email,
                name,
                role: Role::parse(&role),
                created_at,
                disabled_at,
            },
        )
        .collect())
}

/// Disabling ends every session that operator holds, in the same transaction.
/// A disabled account that keeps its sessions is not disabled.
pub async fn set_disabled(pool: &PgPool, id: Uuid, disabled: bool) -> tezgah::Result<()> {
    let mut tx = pool.begin().await?;

    let changed = sqlx::query(
        "update server_operator
         set disabled_at = case when $2 then now() else null end
         where id = $1",
    )
    .bind(id)
    .bind(disabled)
    .execute(&mut *tx)
    .await?;

    if changed.rows_affected() == 0 {
        return Err(tezgah::Error::not_found("operator"));
    }

    if disabled {
        sqlx::query("delete from server_session where operator_id = $1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

/// Changing a password ends every session but the one asking, so a stolen
/// token does not outlive the moment it was noticed.
pub async fn change_password(
    pool: &PgPool,
    id: Uuid,
    password: &str,
    keep: Option<&str>,
) -> tezgah::Result<()> {
    if password.chars().count() < 12 {
        return Err(tezgah::Error::invalid(
            "a password is at least twelve characters",
        ));
    }

    let hash = hash_password(password)?;
    let mut tx = pool.begin().await?;

    let changed = sqlx::query("update server_operator set password_hash = $2 where id = $1")
        .bind(id)
        .bind(&hash)
        .execute(&mut *tx)
        .await?;

    if changed.rows_affected() == 0 {
        return Err(tezgah::Error::not_found("operator"));
    }

    sqlx::query(
        "delete from server_session
         where operator_id = $1 and ($2::bytea is null or token_digest <> $2)",
    )
    .bind(id)
    .bind(keep.map(digest))
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

/// The same answer — `denied` — for an address nobody holds and a password
/// that is wrong, so this cannot be asked which addresses exist.
pub async fn sign_in(pool: &PgPool, email: &str, password: &str) -> tezgah::Result<IssuedSession> {
    let found: Option<CredentialTuple> = sqlx::query_as(
        "select id, email, name, password_hash, role, disabled_at
         from server_operator
         where lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    let Some((id, email, name, hash, role, disabled_at)) = found else {
        // An argon2 hash is run anyway, and discarded. Verifying against a
        // stored hash costs the same work as producing one, so an address
        // nobody holds takes as long to refuse as a password that is wrong —
        // which is the oracle this is trying not to be.
        let _ = hash_password(password);
        return Err(tezgah::Error::denied());
    };

    if disabled_at.is_some() || !password_matches(password, &hash) {
        return Err(tezgah::Error::denied());
    }

    let token = mint_token();
    let expires_at = Utc::now() + Duration::days(SESSION_DAYS);

    sqlx::query(
        "insert into server_session (id, operator_id, token_digest, expires_at)
         values ($1, $2, $3, $4)",
    )
    .bind(Uuid::now_v7())
    .bind(id)
    .bind(digest(&token))
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(IssuedSession {
        token,
        expires_at,
        operator: Operator {
            id,
            email,
            name,
            role: Role::parse(&role),
        },
    })
}

/// Who is holding this token, if anybody still is.
///
/// One statement: the expiry is checked in the `where`, and `last_seen_at` is
/// written by the same `update ... returning`, so there is no window between
/// reading a session and finding it already gone.
pub async fn session_operator(pool: &PgPool, token: &str) -> tezgah::Result<Option<Operator>> {
    let found: Option<(Uuid, String, String, String)> = sqlx::query_as(
        "update server_session s
         set last_seen_at = now()
         from server_operator o
         where s.token_digest = $1
           and s.expires_at > now()
           and o.id = s.operator_id
           and o.disabled_at is null
         returning o.id, o.email, o.name, o.role",
    )
    .bind(digest(token))
    .fetch_optional(pool)
    .await?;

    Ok(found.map(|(id, email, name, role)| Operator {
        id,
        email,
        name,
        role: Role::parse(&role),
    }))
}

pub async fn sign_out(pool: &PgPool, token: &str) -> tezgah::Result<()> {
    sqlx::query("delete from server_session where token_digest = $1")
        .bind(digest(token))
        .execute(pool)
        .await?;
    Ok(())
}

/// Expired rows are not read by anything, and a table nobody sweeps grows for
/// ever. Called by the scheduler beside the shop's own sweeps.
pub async fn drop_expired_sessions(pool: &PgPool) -> tezgah::Result<u64> {
    let gone = sqlx::query("delete from server_session where expires_at <= now()")
        .execute(pool)
        .await?;
    Ok(gone.rows_affected())
}

/// `ADMIN_TOKEN`, checked the way it always was — in constant time, so a
/// byte-by-byte timing difference cannot be used to guess it.
pub fn is_admin_token(presented: &str, expected: &str) -> bool {
    presented.len() == expected.len() && bool::from(presented.as_bytes().ct_eq(expected.as_bytes()))
}

/// Changing what somebody may do. Their sessions survive: a role is read off
/// the operator row on every request, so a narrowed operator is narrowed on
/// their next one rather than at their next sign-in.
pub async fn set_role(pool: &PgPool, id: Uuid, role: Role) -> tezgah::Result<()> {
    let changed = sqlx::query("update server_operator set role = $2 where id = $1")
        .bind(id)
        .bind(role.as_str())
        .execute(pool)
        .await?;

    if changed.rows_affected() == 0 {
        return Err(tezgah::Error::not_found("operator"));
    }
    Ok(())
}

/// How many owners are left, asked before one is taken away.
pub async fn owners(pool: &PgPool) -> tezgah::Result<i64> {
    let (total,): (i64,) = sqlx::query_as(
        "select count(*) from server_operator where role = 'owner' and disabled_at is null",
    )
    .fetch_one(pool)
    .await?;
    Ok(total)
}
