//! `tezgah-server seed` must be safe to run twice against the same shop.
//!
//! Before this test existed, a second run hit `region`'s own unique index
//! (`region_name_key`, `scope, name` — an index, not a `pg_constraint` row,
//! which is why grepping the catalogue for a constraint on `region` found
//! none) with a plain `insert`, which raises a raw `23505` and aborts the
//! transaction before `create_sales_channel`'s own conflict handling — the
//! one path `seed::run` actually caught — is ever reached. The operator saw
//! "the database refused a query" instead of "already seeded".
//!
//! This runs against a real, disposable Postgres — `DATABASE_URL`, or the
//! same default the workspace's own `tests/common` falls back to — and
//! counts the rows a second run left behind rather than reading the code and
//! trusting it.

use sqlx::postgres::PgPoolOptions;
use sqlx::{Executor, PgPool};
use tezgah::ports::Scope;
use tezgah_server::host::ServerHost;
use tezgah_server::seed;
use uuid::Uuid;

struct Database {
    admin: PgPool,
    pool: PgPool,
    name: String,
}

impl Database {
    async fn fresh() -> Database {
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/postgres".into());

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("a Postgres to test against");

        let name = format!("tezgah_server_seed_{}", Uuid::now_v7().simple());
        admin
            .execute(format!(r#"create database "{name}""#).as_str())
            .await
            .expect("a database of its own");

        let mut its_url = url::Url::parse(&url).expect("a database url");
        its_url.set_path(&name);

        let pool = PgPoolOptions::new()
            .max_connections(4)
            .connect(its_url.as_str())
            .await
            .expect("its own database");

        tezgah::MIGRATIONS
            .run(&pool)
            .await
            .expect("the migrations to apply");

        // Seeding a shop writes an audit row, and `server_audit` is not one
        // of tezgah's tables.
        tezgah_server::prepare(&pool)
            .await
            .expect("the tables this binary owns");

        Database { admin, pool, name }
    }

    async fn close(self) {
        self.pool.close().await;
        self.admin
            .execute(format!(r#"drop database "{}" with (force)"#, self.name).as_str())
            .await
            .expect("to clean up the database this test made");
    }
}

async fn seeded_row_counts(pool: &PgPool) -> (i64, i64, i64, i64) {
    let region: i64 = sqlx::query_scalar("select count(*) from region")
        .fetch_one(pool)
        .await
        .expect("region is queryable");
    let channel: i64 = sqlx::query_scalar("select count(*) from sales_channel")
        .fetch_one(pool)
        .await
        .expect("sales_channel is queryable");
    let location: i64 = sqlx::query_scalar("select count(*) from stock_location")
        .fetch_one(pool)
        .await
        .expect("stock_location is queryable");
    let key: i64 = sqlx::query_scalar("select count(*) from publishable_key")
        .fetch_one(pool)
        .await
        .expect("publishable_key is queryable");
    (region, channel, location, key)
}

#[tokio::test]
async fn seed_run_twice_writes_nothing_the_second_time() {
    let db = Database::fresh().await;

    let scope = Scope(Uuid::now_v7());
    sqlx::query("insert into tezgah_scope (id) values ($1)")
        .bind(scope.0)
        .execute(&db.pool)
        .await
        .expect("a scope to seed into");

    let host = ServerHost;

    seed::run(&db.pool, scope, &host, 2)
        .await
        .expect("the first run seeds a shop");
    let after_first = seeded_row_counts(&db.pool).await;
    assert_eq!(
        after_first,
        (1, 1, 1, 1),
        "one region, sales channel, stock location and publishable key"
    );

    seed::run(&db.pool, scope, &host, 2)
        .await
        .expect("a second run against the same shop must not error");
    let after_second = seeded_row_counts(&db.pool).await;
    assert_eq!(
        after_second, after_first,
        "a second run wrote rows the first run already wrote"
    );

    db.close().await;
}
