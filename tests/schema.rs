//! What the migrations must be true of, asked of a database they actually ran
//! against.
//!
//! These are generated rather than written one per table: a rule enforced by a
//! test somebody has to remember to add is a rule that lasts until the next
//! table.

mod common;

use common::Shop;

#[tokio::test]
async fn the_migrations_apply_to_an_empty_database() {
    let shop = Shop::open().await;

    let tables: i64 = sqlx::query_scalar("select count(*) from tezgah_table")
        .fetch_one(&shop.pool)
        .await
        .expect("the registry to be readable");

    assert!(
        tables > 40,
        "only {tables} tables registered; a migration is not calling tezgah_register"
    );

    shop.close().await;
}

/// The registry's own tables and sqlx's bookkeeping. Nothing else belongs
/// here: a table outside it is one no policy guards and no test looks at.
/// Tables that carry no scope on purpose: the scope registry itself, the table
/// registry, sqlx's own, and the order transitions, which are the library's
/// rules rather than a shop's.
const UNSCOPED: [&str; 4] = [
    "tezgah_scope",
    "tezgah_table",
    "_sqlx_migrations",
    "tezgah_order_status_move",
];

#[tokio::test]
async fn no_table_exists_that_the_registry_has_never_heard_of() {
    let shop = Shop::open().await;

    let strays: Vec<String> = sqlx::query_scalar(
        "select c.relname::text
         from pg_class c
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         where c.relkind = 'r'
           and not exists (select 1 from tezgah_table t where t.name = c.relname)
           and c.relname <> all($1)
         order by 1",
    )
    .bind(UNSCOPED.to_vec())
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_class");

    assert!(
        strays.is_empty(),
        "these tables exist but never called tezgah_register, so they carry no scope, \
         no policy and no test: {strays:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn every_registered_table_forces_row_level_security() {
    let shop = Shop::open().await;

    let loose: Vec<String> = sqlx::query_scalar(
        "select t.name
         from tezgah_table t
         join pg_class c on c.relname = t.name
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         where not c.relrowsecurity or not c.relforcerowsecurity
         order by t.name",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_class");

    assert!(
        loose.is_empty(),
        "these tables do not force row-level security: {loose:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn every_registered_table_has_a_scope_policy() {
    let shop = Shop::open().await;

    let unguarded: Vec<String> = sqlx::query_scalar(
        "select t.name
         from tezgah_table t
         where not exists (
             select 1 from pg_policies p
             where p.schemaname = 'public' and p.tablename = t.name
         )
         order by t.name",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_policies");

    assert!(
        unguarded.is_empty(),
        "these tables have row-level security on but no policy, so they admit nothing: {unguarded:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn every_foreign_key_is_indexed() {
    let shop = Shop::open().await;

    // An unindexed foreign key turns deleting the parent into a scan of the
    // child, which is fine on a hundred rows and not on a million.
    let bare: Vec<(String, String)> = sqlx::query_as(
        r#"
        with fk as (
            select
                con.conrelid as tbl,
                con.conname  as name,
                con.conkey   as cols
            from pg_constraint con
            join pg_class c on c.oid = con.conrelid
            join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
            where con.contype = 'f'
        )
        select c.relname::text, fk.name::text
        from fk
        join pg_class c on c.oid = fk.tbl
        where not exists (
            select 1
            from pg_index i
            where i.indrelid = fk.tbl
              and (fk.cols::smallint[]) <@ (i.indkey::smallint[])
        )
        order by 1, 2
        "#,
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_constraint");

    assert!(
        bare.is_empty(),
        "these foreign keys have no index covering them: {bare:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn no_table_stores_money_without_saying_which() {
    let shop = Shop::open().await;

    // An amount without a currency beside it is a number nobody can spend.
    let orphans: Vec<(String, String)> = sqlx::query_as(
        "select c.table_name::text, c.column_name::text
         from information_schema.columns c
         join tezgah_table t on t.name = c.table_name
         where c.table_schema = 'public'
           and c.data_type = 'numeric'
           and (c.column_name like '%amount%' or c.column_name like '%price%'
                or c.column_name like '%total%' or c.column_name like '%subtotal%')
           and not exists (
               select 1 from information_schema.columns k
               where k.table_schema = 'public'
                 and k.table_name = c.table_name
                 and k.column_name = 'currency_code'
           )
         order by 1, 2",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read information_schema");

    assert!(
        orphans.is_empty(),
        "these money columns have no currency beside them: {orphans:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn no_column_holds_a_time_without_a_zone() {
    let shop = Shop::open().await;

    let naive: Vec<(String, String)> = sqlx::query_as(
        "select c.table_name::text, c.column_name::text
         from information_schema.columns c
         join tezgah_table t on t.name = c.table_name
         where c.table_schema = 'public'
           and c.data_type in ('timestamp without time zone', 'time without time zone')
         order by 1, 2",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read information_schema");

    assert!(
        naive.is_empty(),
        "these columns hold a time with no zone: {naive:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn no_money_is_kept_as_a_float() {
    let shop = Shop::open().await;

    let floats: Vec<(String, String)> = sqlx::query_as(
        "select c.table_name::text, c.column_name::text
         from information_schema.columns c
         join tezgah_table t on t.name = c.table_name
         where c.table_schema = 'public'
           and c.data_type in ('real', 'double precision')
         order by 1, 2",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read information_schema");

    assert!(floats.is_empty(), "these columns are floats: {floats:?}");

    shop.close().await;
}

#[tokio::test]
async fn a_row_written_in_one_scope_is_invisible_in_another() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    sqlx::query(
        "insert into workflow_run (id, scope, name, transaction_key)
         values ($1, $2, 'checkout', 'k')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *mine)
    .await
    .expect("to write in my own scope");
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let seen: i64 = sqlx::query_scalar("select count(*) from workflow_run")
        .fetch_one(&mut *theirs)
        .await
        .expect("to count");
    theirs.commit().await.expect("to commit");

    assert_eq!(seen, 0, "another scope could see the row");

    shop.close().await;
}

#[tokio::test]
async fn a_connection_that_names_no_scope_sees_nothing() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    sqlx::query(
        "insert into workflow_run (id, scope, name, transaction_key)
         values ($1, $2, 'checkout', 'k')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.here.0)
    .execute(&mut *mine)
    .await
    .expect("to write");
    mine.commit().await.expect("to commit");

    // Forgetting is the dangerous case: it must be blindness, never everyone.
    let seen: i64 = sqlx::query_scalar("select count(*) from workflow_run")
        .fetch_one(&shop.pool)
        .await
        .expect("to count");

    assert_eq!(seen, 0, "a connection with no scope set could read rows");

    shop.close().await;
}

#[tokio::test]
async fn a_row_cannot_be_written_into_somebody_elses_scope() {
    let shop = Shop::open().await;

    let mut mine = shop.begin().await;
    let refused = sqlx::query(
        "insert into workflow_run (id, scope, name, transaction_key)
         values ($1, $2, 'checkout', 'k')",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.elsewhere.0)
    .execute(&mut *mine)
    .await;

    // Rolled back before the pool is closed: a live transaction holds a
    // connection, and closing a pool waits for every one of them back.
    mine.rollback().await.expect("to give the connection back");

    assert!(
        refused.is_err(),
        "the policy's check clause let a row be written into another scope"
    );

    shop.close().await;
}
