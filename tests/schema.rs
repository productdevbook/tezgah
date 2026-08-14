//! What the migrations must be true of, asked of a database they actually ran
//! against.
//!
//! These are generated rather than written one per table: a rule enforced by a
//! test somebody has to remember to add is a rule that lasts until the next
//! table.

mod common;

use common::Shop;
use sqlx::Executor;

#[tokio::test]
async fn every_registered_table_is_unique_on_scope_and_id() {
    let shop = Shop::open().await;

    // Without this a foreign key cannot name `(scope, id)`, and every
    // reference in the schema is free to cross a tenant boundary.
    let loose: Vec<String> = sqlx::query_scalar(
        "select t.name
         from tezgah_table t
         where not exists (
             select 1
             from pg_constraint con
             join pg_class c on c.oid = con.conrelid and c.relname = t.name
             join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
             where con.contype in ('u', 'p')
               and (select array_agg(a.attname::text order by a.attname)
                    from pg_attribute a
                    where a.attrelid = con.conrelid and a.attnum = any (con.conkey))
                   = array['id', 'scope']
         )
         order by t.name",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_constraint");

    assert!(
        loose.is_empty(),
        "these tables have no unique (scope, id), so nothing can key to them by scope: {loose:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn no_key_on_a_scoped_table_can_cross_a_scope() {
    let shop = Shop::open().await;

    let bare: Vec<(String, String)> = sqlx::query_as(
        "select c.relname::text, con.conname::text
         from tezgah_scoped_fk_table f
         join pg_class c on c.relname = f.name
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         join pg_constraint con on con.conrelid = c.oid and con.contype = 'f'
         where not exists (
             select 1 from pg_attribute a
             where a.attrelid = con.conrelid
               and a.attname = 'scope'
               and a.attnum = any (con.conkey)
         )
         order by 1, 2",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_constraint");

    assert!(
        bare.is_empty(),
        "these keys are single-column on a table whose keys must carry the scope, \
         so Postgres — which checks a foreign key with row security bypassed — \
         would let one shop point at another's row: {bare:?}"
    );

    shop.close().await;
}

/// Registration is what makes the test above ask about a table at all, so a
/// table with no scoped key left is a table nothing checks.
#[tokio::test]
async fn the_catalogue_pricing_inventory_and_cart_tables_are_registered_as_scoped() {
    let shop = Shop::open().await;

    let missing: Vec<String> = sqlx::query_scalar(
        "select t.name
         from unnest(array[
             'product', 'product_variant', 'product_option', 'product_option_value',
             'product_variant_option_value', 'product_image', 'product_tag_link',
             'product_category_link', 'product_sales_channel', 'product_translation',
             'product_category',
             'price', 'price_rule', 'price_list_rule',
             'product_variant_price_set', 'shipping_option_price_set',
             'stock_location', 'stock_location_sales_channel', 'inventory_level',
             'reservation_item', 'variant_inventory_item',
             'cart', 'cart_address', 'cart_line_item', 'cart_line_item_adjustment',
             'cart_line_item_tax_line', 'cart_shipping_method',
             'cart_shipping_method_adjustment', 'cart_shipping_method_tax_line'
         ]) as t(name)
         where not exists (select 1 from tezgah_scoped_fk_table f where f.name = t.name)
         order by 1",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read tezgah_scoped_fk_table");

    assert!(
        missing.is_empty(),
        "these tables carry goods, prices or a shopper's cart and are not held to \
         a scoped key, so nothing would notice one of their keys going single-column \
         again: {missing:?}"
    );

    shop.close().await;
}

#[tokio::test]
async fn nothing_cascades_away_a_record_of_what_happened() {
    let shop = Shop::open().await;

    let doomed: Vec<(String, String)> = sqlx::query_as(
        "select c.relname::text, con.conname::text
         from tezgah_evidence_table e
         join pg_class c on c.relname = e.name
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         join pg_constraint con on con.conrelid = c.oid and con.contype = 'f'
         where con.confdeltype = 'c'
         order by 1, 2",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_constraint");

    assert!(
        doomed.is_empty(),
        "these tables exist to be the record of a thing that happened, and a delete \
         upstream would take them with it without anybody saying so: {doomed:?}"
    );

    shop.close().await;
}

/// 0022's backfills ran under forced row-level security with no `app.scope`,
/// so they matched nothing, and the constraint that followed validated every
/// row regardless — which raises on any database that had one.
///
/// Every other test here runs the migrations against an empty database, which
/// is exactly why nothing saw it. This one writes the row first.
#[tokio::test]
async fn a_migration_backfills_a_database_that_already_has_rows() {
    let shop = Shop::open().await;

    let order = uuid::Uuid::now_v7();
    let returned = uuid::Uuid::now_v7();

    let mut mine = shop.begin().await;
    sqlx::query(r#"insert into "order" (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(order)
        .bind(shop.here.0)
        .execute(&mut *mine)
        .await
        .expect("an order");
    sqlx::query(
        "insert into order_return (id, scope, order_id, order_version, status, currency_code)
         values ($1, $2, $3, 1, 'open', 'TRY')",
    )
    .bind(returned)
    .bind(shop.here.0)
    .bind(order)
    .execute(&mut *mine)
    .await
    .expect("a return");
    mine.commit().await.expect("to commit");

    let owner = shop.migrator().await;

    // The row as it was legal to write before 0022: canceled, with no moment.
    owner
        .execute("alter table order_return drop constraint order_return_canceled_valid")
        .await
        .expect("to put the table back as 0022 found it");

    let mut mine = shop.begin().await;
    sqlx::query(
        "update order_return set status = 'canceled', canceled_at = null
         where scope = $1 and id = $2",
    )
    .bind(shop.here.0)
    .bind(returned)
    .execute(&mut *mine)
    .await
    .expect("to cancel it");
    mine.commit().await.expect("to commit");

    // 0022's backfill verbatim, as a migration runs it.
    let blind = sqlx::query(
        "update order_return set canceled_at = now()
         where status = 'canceled' and canceled_at is null",
    )
    .execute(&owner)
    .await
    .expect("to run");

    assert_eq!(
        blind.rows_affected(),
        0,
        "a migration reached a row without naming a scope, so this test no longer \
         proves what 0024 is for"
    );

    // 0024, which names each scope and then validates what 0022 added blind.
    owner
        .execute(include_str!("../migrations/0024_order_status_backfill.sql"))
        .await
        .expect("0024 to apply to a database that has rows in it");

    let mut mine = shop.begin().await;
    let stamped: Option<chrono::DateTime<chrono::Utc>> =
        sqlx::query_scalar("select canceled_at from order_return where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(returned)
            .fetch_one(&mut *mine)
            .await
            .expect("to read the return back");
    mine.commit().await.expect("to commit");

    assert!(
        stamped.is_some(),
        "the backfill did not reach the row it was written for"
    );

    owner.close().await;
    shop.close().await;
}

/// 0046 replaces `metadata->>'subscription'` with a real column. A database
/// that ran the old convention has orders carrying only the JSON key, and the
/// backfill has to move a well-formed, same-scope one across while leaving a
/// malformed key, or one naming another shop's contract, with no link rather
/// than one the new foreign key would refuse.
#[tokio::test]
async fn a_migration_backfills_the_subscription_link_from_metadata() {
    let shop = Shop::open().await;

    async fn a_plan(tx: &mut sqlx::PgConnection, scope: uuid::Uuid) -> uuid::Uuid {
        let group = uuid::Uuid::now_v7();
        let plan = uuid::Uuid::now_v7();
        sqlx::query("insert into selling_plan_group (id, scope, name) values ($1, $2, 'a group')")
            .bind(group)
            .bind(scope)
            .execute(&mut *tx)
            .await
            .expect("a plan group");
        sqlx::query(
            "insert into selling_plan
                 (id, scope, selling_plan_group_id, name, billing_interval_unit,
                  billing_interval_count)
             values ($1, $2, $3, 'monthly', 'month', 1)",
        )
        .bind(plan)
        .bind(scope)
        .bind(group)
        .execute(&mut *tx)
        .await
        .expect("a plan");
        plan
    }

    async fn a_subscription(
        tx: &mut sqlx::PgConnection,
        scope: uuid::Uuid,
        plan: uuid::Uuid,
    ) -> uuid::Uuid {
        let customer = uuid::Uuid::now_v7();
        let subscription = uuid::Uuid::now_v7();
        sqlx::query("insert into customer (id, scope) values ($1, $2)")
            .bind(customer)
            .bind(scope)
            .execute(&mut *tx)
            .await
            .expect("a customer");
        sqlx::query(
            "insert into subscription
                 (id, scope, customer_id, selling_plan_id, currency_code, next_billing_at,
                  current_period_start, current_period_end)
             values ($1, $2, $3, $4, 'TRY', now(), now(), now())",
        )
        .bind(subscription)
        .bind(scope)
        .bind(customer)
        .bind(plan)
        .execute(&mut *tx)
        .await
        .expect("a contract");
        subscription
    }

    let mut mine = shop.begin().await;
    let plan_here = a_plan(&mut mine, shop.here.0).await;
    let contract_here = a_subscription(&mut mine, shop.here.0, plan_here).await;

    let real = uuid::Uuid::now_v7();
    let malformed = uuid::Uuid::now_v7();
    let none = uuid::Uuid::now_v7();

    sqlx::query(
        r#"insert into "order" (id, scope, currency_code, metadata) values ($1, $2, 'TRY', $3)"#,
    )
    .bind(real)
    .bind(shop.here.0)
    .bind(serde_json::json!({ "subscription": contract_here }))
    .execute(&mut *mine)
    .await
    .expect("an order under the old convention");

    sqlx::query(
        r#"insert into "order" (id, scope, currency_code, metadata) values ($1, $2, 'TRY', $3)"#,
    )
    .bind(malformed)
    .bind(shop.here.0)
    .bind(serde_json::json!({ "subscription": "not-a-uuid" }))
    .execute(&mut *mine)
    .await
    .expect("an order with a malformed key");

    sqlx::query(r#"insert into "order" (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(none)
        .bind(shop.here.0)
        .execute(&mut *mine)
        .await
        .expect("an order with no subscription at all");
    mine.commit().await.expect("to commit");

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    let plan_elsewhere = a_plan(&mut theirs, shop.elsewhere.0).await;
    let contract_elsewhere = a_subscription(&mut theirs, shop.elsewhere.0, plan_elsewhere).await;
    theirs.commit().await.expect("to commit");

    let elsewhere = uuid::Uuid::now_v7();
    let mut mine = shop.begin().await;
    sqlx::query(
        r#"insert into "order" (id, scope, currency_code, metadata) values ($1, $2, 'TRY', $3)"#,
    )
    .bind(elsewhere)
    .bind(shop.here.0)
    .bind(serde_json::json!({ "subscription": contract_elsewhere }))
    .execute(&mut *mine)
    .await
    .expect("an order naming another shop's contract");
    mine.commit().await.expect("to commit");

    // These four rows are exactly what a database that ran the old
    // convention looks like: `subscription_id` was never set on them, since
    // nothing but 0046's own backfill writes it. Running 0046 again is what
    // asks whether that backfill does the right thing with each of them.
    let owner = shop.migrator().await;
    owner
        .execute(include_str!(
            "../migrations/0046_order_subscription_link.sql"
        ))
        .await
        .expect("0046 to apply to a database that already has orders in it");

    let mut mine = shop.begin().await;
    let linked: (
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
        Option<uuid::Uuid>,
    ) = (
        sqlx::query_scalar("select subscription_id from \"order\" where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(real)
            .fetch_one(&mut *mine)
            .await
            .expect("the real row"),
        sqlx::query_scalar("select subscription_id from \"order\" where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(malformed)
            .fetch_one(&mut *mine)
            .await
            .expect("the malformed row"),
        sqlx::query_scalar("select subscription_id from \"order\" where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(none)
            .fetch_one(&mut *mine)
            .await
            .expect("the unlinked row"),
        sqlx::query_scalar("select subscription_id from \"order\" where scope = $1 and id = $2")
            .bind(shop.here.0)
            .bind(elsewhere)
            .fetch_one(&mut *mine)
            .await
            .expect("the cross-scope row"),
    );
    mine.commit().await.expect("to commit");

    assert_eq!(
        linked.0,
        Some(contract_here),
        "a well-formed, same-scope key was not moved to the column"
    );
    assert_eq!(linked.1, None, "a malformed key was moved anyway");
    assert_eq!(
        linked.2, None,
        "an order with no key gained a link from nowhere"
    );
    assert_eq!(
        linked.3, None,
        "a key naming another shop's contract was moved anyway"
    );

    owner.close().await;
    shop.close().await;
}

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
const UNSCOPED: [&str; 6] = [
    "tezgah_scope",
    "tezgah_table",
    "_sqlx_migrations",
    "tezgah_order_status_move",
    "tezgah_evidence_table",
    "tezgah_scoped_fk_table",
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

/// A parent in one shop, a child in another, written straight past the domain
/// code. Postgres checks a foreign key with row security bypassed, so a
/// single-column key is a hole nothing else can close: it is real to the
/// constraint and invisible to every read.
#[tokio::test]
async fn a_key_cannot_name_another_shops_row() {
    let shop = Shop::open().await;

    let campaign = uuid::Uuid::now_v7();
    let customer = uuid::Uuid::now_v7();
    let region = uuid::Uuid::now_v7();
    let set = uuid::Uuid::now_v7();
    let location = uuid::Uuid::now_v7();
    let parcel = uuid::Uuid::now_v7();
    let plan_group = uuid::Uuid::now_v7();
    let plan = uuid::Uuid::now_v7();
    let subscription = uuid::Uuid::now_v7();

    let mut theirs = shop.begin_as(shop.elsewhere).await;
    for (sql, id) in [
        (
            "insert into campaign (id, scope, identifier, name) values ($1, $2, 'c', 'a campaign')",
            campaign,
        ),
        ("insert into customer (id, scope) values ($1, $2)", customer),
        (
            "insert into tax_region (id, scope, country_code) values ($1, $2, 'TR')",
            region,
        ),
        (
            "insert into fulfillment_set (id, scope, name, type) values ($1, $2, 'delivery', 'shipping')",
            set,
        ),
        (
            "insert into stock_location (id, scope, name) values ($1, $2, 'a warehouse')",
            location,
        ),
        (
            "insert into selling_plan_group (id, scope, name) values ($1, $2, 'a group')",
            plan_group,
        ),
    ] {
        sqlx::query(sql)
            .bind(id)
            .bind(shop.elsewhere.0)
            .execute(&mut *theirs)
            .await
            .expect("the other shop to seed its own rows");
    }
    sqlx::query("insert into fulfillment (id, scope, location_id) values ($1, $2, $3)")
        .bind(parcel)
        .bind(shop.elsewhere.0)
        .bind(location)
        .execute(&mut *theirs)
        .await
        .expect("the other shop to open a parcel");
    sqlx::query(
        "insert into selling_plan
             (id, scope, selling_plan_group_id, name, billing_interval_unit, billing_interval_count)
         values ($1, $2, $3, 'monthly', 'month', 1)",
    )
    .bind(plan)
    .bind(shop.elsewhere.0)
    .bind(plan_group)
    .execute(&mut *theirs)
    .await
    .expect("the other shop to open a selling plan");
    sqlx::query(
        "insert into subscription
             (id, scope, customer_id, selling_plan_id, currency_code, next_billing_at,
              current_period_start, current_period_end)
         values ($1, $2, $3, $4, 'TRY', now(), now(), now())",
    )
    .bind(subscription)
    .bind(shop.elsewhere.0)
    .bind(customer)
    .bind(plan)
    .execute(&mut *theirs)
    .await
    .expect("the other shop to open a contract");
    theirs.commit().await.expect("to commit");

    // The two already on `tezgah_evidence_table` come first: the schema
    // already says these rows must survive a delete, and a bare key there let
    // a protected row point across a tenant boundary anyway.
    for (what, sql, parent) in [
        (
            "fulfillment_label.fulfillment_id",
            "insert into fulfillment_label (id, scope, fulfillment_id, tracking_number)
             values ($1, $2, $3, 'TRACK-1')",
            parcel,
        ),
        (
            "campaign_budget.campaign_id",
            "insert into campaign_budget (id, scope, campaign_id, type)
             values ($1, $2, $3, 'usage')",
            campaign,
        ),
        (
            "customer_address.customer_id",
            "insert into customer_address (id, scope, customer_id) values ($1, $2, $3)",
            customer,
        ),
        (
            "tax_rate.tax_region_id",
            "insert into tax_rate (id, scope, tax_region_id, rate, name)
             values ($1, $2, $3, 20, 'vat')",
            region,
        ),
        (
            "service_zone.fulfillment_set_id",
            "insert into service_zone (id, scope, name, fulfillment_set_id)
             values ($1, $2, 'a zone', $3)",
            set,
        ),
        (
            "fulfillment.location_id",
            "insert into fulfillment (id, scope, location_id) values ($1, $2, $3)",
            location,
        ),
        (
            "order.subscription_id",
            "insert into \"order\" (id, scope, currency_code, subscription_id)
             values ($1, $2, 'TRY', $3)",
            subscription,
        ),
    ] {
        let mut mine = shop.begin().await;
        let refused = sqlx::query(sql)
            .bind(uuid::Uuid::now_v7())
            .bind(shop.here.0)
            .bind(parent)
            .execute(&mut *mine)
            .await;
        mine.rollback().await.expect("to give the connection back");

        let code = refused
            .as_ref()
            .err()
            .and_then(|e| e.as_database_error())
            .and_then(|e| e.code())
            .map(|c| c.to_string());

        assert_eq!(
            code.as_deref(),
            Some("23503"),
            "{what} accepted a parent in another shop: {refused:?}"
        );
    }

    shop.close().await;
}

/// Which physical lot went into which parcel is the row a recall is answered
/// through, so deleting what it hangs off has to be said out loud.
#[tokio::test]
async fn a_shipped_lot_cannot_be_deleted_out_from_under_its_parcel() {
    let shop = Shop::open().await;

    let location = uuid::Uuid::now_v7();
    let item = uuid::Uuid::now_v7();
    let parcel = uuid::Uuid::now_v7();
    let packed = uuid::Uuid::now_v7();
    let lot = uuid::Uuid::now_v7();

    let mut mine = shop.begin().await;
    for (sql, id) in [
        (
            "insert into stock_location (id, scope, name) values ($1, $2, 'a warehouse')",
            location,
        ),
        (
            "insert into inventory_item (id, scope, sku) values ($1, $2, 'SKU-1')",
            item,
        ),
    ] {
        sqlx::query(sql)
            .bind(id)
            .bind(shop.here.0)
            .execute(&mut *mine)
            .await
            .expect("a shelf to ship off");
    }

    sqlx::query("insert into fulfillment (id, scope, location_id) values ($1, $2, $3)")
        .bind(parcel)
        .bind(shop.here.0)
        .bind(location)
        .execute(&mut *mine)
        .await
        .expect("a parcel");
    sqlx::query(
        "insert into fulfillment_item (id, scope, fulfillment_id, title, quantity)
         values ($1, $2, $3, 'a thing', 1)",
    )
    .bind(packed)
    .bind(shop.here.0)
    .bind(parcel)
    .execute(&mut *mine)
    .await
    .expect("something in the box");
    sqlx::query(
        "insert into inventory_lot (id, scope, inventory_item_id, location_id, lot_code)
         values ($1, $2, $3, $4, 'LOT-1')",
    )
    .bind(lot)
    .bind(shop.here.0)
    .bind(item)
    .bind(location)
    .execute(&mut *mine)
    .await
    .expect("a lot");
    sqlx::query(
        "insert into fulfillment_lot
             (id, scope, fulfillment_item_id, inventory_lot_id, lot_code, quantity)
         values ($1, $2, $3, $4, 'LOT-1', 1)",
    )
    .bind(uuid::Uuid::now_v7())
    .bind(shop.here.0)
    .bind(packed)
    .bind(lot)
    .execute(&mut *mine)
    .await
    .expect("the lot to go in the box");

    let swept = sqlx::query("delete from fulfillment_item where scope = $1 and id = $2")
        .bind(shop.here.0)
        .bind(packed)
        .execute(&mut *mine)
        .await;
    mine.rollback().await.expect("to give the connection back");

    let code = swept
        .as_ref()
        .err()
        .and_then(|e| e.as_database_error())
        .and_then(|e| e.code())
        .map(|c| c.to_string());

    // 23001 rather than 23503: `restrict` is checked before the row goes, and
    // Postgres reports that as its own thing.
    assert_eq!(
        code.as_deref(),
        Some("23001"),
        "deleting the item took the answer to \"who received this batch\" with it: {swept:?}"
    );

    shop.close().await;
}
