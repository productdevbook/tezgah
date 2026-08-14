//! Isolation, asked of every registered table rather than of one somebody
//! remembered to write a test for.
//!
//! A row is written in one scope, and a second scope is then asked to see it,
//! change it and delete it. All three must come back empty. The tables and the
//! columns come out of the catalogue, so a table added tomorrow is covered the
//! day it is added.

mod common;

use std::collections::HashMap;

use common::Shop;
use sqlx::{Row, postgres::PgRow};
use uuid::Uuid;

/// A column as the catalogue describes it.
struct Column {
    name: String,
    data_type: String,
    /// What one element is, for an array column: `_text` becomes `text`.
    element: String,
    length: Option<i32>,
    required: bool,
}

/// What a table needs before a row can be put in it.
struct Table {
    name: String,
    columns: Vec<Column>,
    /// Column -> the table its value must already exist in.
    parents: HashMap<String, String>,
    /// Column -> a literal a check constraint will accept.
    literals: HashMap<String, String>,
    /// This table's check constraints, casts stripped.
    checks: Vec<String>,
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// The tables the registry knows about, with everything needed to seed one.
async fn describe(shop: &Shop) -> Vec<Table> {
    let names: Vec<String> = sqlx::query_scalar("select name from tezgah_table order by name")
        .fetch_all(&shop.pool)
        .await
        .expect("the registry to be readable");

    let columns: Vec<PgRow> = sqlx::query(
        "select table_name::text, column_name::text, data_type::text,
                character_maximum_length::int,
                (is_nullable = 'NO' and column_default is null) as required,
                udt_name::text
         from information_schema.columns
         where table_schema = 'public'
         order by ordinal_position",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read information_schema.columns");

    // Foreign keys pointing at a primary key called `id`, whether they name
    // the scope alongside it or not; the rest are left to fail the insert and
    // be reported as uncovered.
    let keys: Vec<(String, String, String, String)> = sqlx::query_as(
        "select c.relname::text, a.attname::text, f.relname::text, fa.attname::text
         from pg_constraint con
         join pg_class c on c.oid = con.conrelid
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         join pg_class f on f.oid = con.confrelid
         join unnest(con.conkey) with ordinality as k(att, ord) on true
         join pg_attribute a on a.attrelid = con.conrelid and a.attnum = k.att
         join unnest(con.confkey) with ordinality as fk(att, ord) on fk.ord = k.ord
         join pg_attribute fa on fa.attrelid = con.confrelid and fa.attnum = fk.att
         where con.contype = 'f'
           and fa.attname = 'id'
           and a.attname <> 'scope'
           and (cardinality(con.conkey) = 1
                or exists (
                    select 1 from pg_attribute s
                    where s.attrelid = con.conrelid
                      and s.attname = 'scope'
                      and s.attnum = any (con.conkey)
                ))",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read pg_constraint");

    let checks: Vec<(String, String)> = sqlx::query_as(
        "select c.relname::text, pg_get_constraintdef(con.oid)::text
         from pg_constraint con
         join pg_class c on c.oid = con.conrelid
         join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
         where con.contype = 'c'",
    )
    .fetch_all(&shop.pool)
    .await
    .expect("to read check constraints");

    let mut tables = Vec::new();

    for name in names {
        let columns = columns
            .iter()
            .filter(|row| row.get::<String, _>(0) == name)
            .map(|row| Column {
                name: row.get(1),
                data_type: row.get(2),
                element: row.get::<String, _>(5).trim_start_matches('_').to_string(),
                length: row.get(3),
                required: row.get(4),
            })
            .collect::<Vec<_>>();

        let parents = keys
            .iter()
            .filter(|(child, _, _, referenced)| *child == name && referenced == "id")
            .map(|(_, column, parent, _)| (column.clone(), parent.clone()))
            .collect();

        let mine: Vec<String> = checks
            .iter()
            .filter(|(table, _)| *table == name)
            .map(|(_, def)| strip_casts(def))
            .collect();

        let mut literals: HashMap<String, String> = HashMap::new();
        for column in &columns {
            let said = mine
                .iter()
                .filter(|def| mentions(def, &column.name))
                .find_map(|def| accepted_literal(def));

            let value = match said {
                Some(literal) => Some(literal),
                None => {
                    let pattern = mine
                        .iter()
                        .filter(|def| mentions(def, &column.name))
                        .find_map(|def| regex_in(def));
                    match pattern {
                        Some(pattern) => matching(&shop.pool, &pattern, column).await,
                        None => None,
                    }
                }
            };

            if let Some(value) = value {
                literals.insert(column.name.clone(), value);
            }
        }

        tables.push(Table {
            name,
            columns,
            parents,
            literals,
            checks: mine,
        });
    }

    tables
}

/// `(locale)::text ~ '^[a-z]{2}'::text` reads as `locale ~ '^[a-z]{2}'`, which
/// is what the patterns below are written against.
fn strip_casts(def: &str) -> String {
    let mut out = String::with_capacity(def.len());
    let mut rest = def;
    while let Some(at) = rest.find("::") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let end = after
            .find(|c: char| !(c.is_alphanumeric() || c == '_' || c == ' '))
            .unwrap_or(after.len());
        rest = after[end..].strip_prefix("[]").unwrap_or(&after[end..]);
    }
    out.push_str(rest);
    out
}

/// The regular expression a check holds, if it holds one.
fn regex_in(def: &str) -> Option<String> {
    let (_, rest) = def.split_once("~ '")?;
    let (pattern, _) = rest.split_once('\'')?;
    Some(pattern.to_string())
}

/// A value the pattern accepts, asked of Postgres rather than guessed: its
/// regular expressions are its own, and a Rust one agreeing is not the point.
///
/// The column's own default is offered first, so a column that already seeded
/// keeps the value it seeded with.
async fn matching(pool: &sqlx::PgPool, pattern: &str, column: &Column) -> Option<String> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(default) = plain_text(column) {
        candidates.push(default);
    }
    candidates.extend(["US", "USD", "en", "en-US", "tezgah"].map(str::to_string));

    for candidate in candidates {
        let accepted: bool = sqlx::query_scalar("select $1 ~ $2")
            .bind(&candidate)
            .bind(pattern)
            .fetch_one(pool)
            .await
            .expect("Postgres to answer whether a value matches");
        if accepted {
            return Some(candidate);
        }
    }
    None
}

/// The text this column would get from its type alone, unquoted.
fn plain_text(column: &Column) -> Option<String> {
    match column.data_type.as_str() {
        "text" | "character varying" | "character" => Some(match column.length {
            Some(2) => "US".into(),
            Some(3) => "USD".into(),
            _ => "tezgah".into(),
        }),
        _ => None,
    }
}

/// The column an emptiness check insists on having something in:
/// `cardinality(allowed_values) > 0`.
fn non_empty_array(def: &str) -> Option<&str> {
    if !def.contains("> 0") {
        return None;
    }
    let (_, rest) = def
        .split_once("cardinality(")
        .or_else(|| def.split_once("array_length("))?;
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(&rest[..end])
}

/// The scalar and the array of `default_currency_code = ANY (supported_currency_codes)`.
/// A literal list — `= ANY (ARRAY[...])` — is somebody else's job.
fn membership(def: &str) -> Option<(&str, &str)> {
    let at = def.find(" = ANY (")?;
    let scalar = ident_before(&def[..at])?;
    let rest = def[at + " = ANY (".len()..].trim_start_matches(['(', ' ']);
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    let array = &rest[..end];
    (!array.is_empty() && array != "ARRAY").then_some((scalar, array))
}

/// `type <> 'fixed' OR currency_code IS NOT NULL` — the column, the value that
/// triggers it, and what then has to be filled in.
fn implication(def: &str) -> Option<(&str, &str, &str)> {
    let at = def.find(" <> '")?;
    let column = ident_before(&def[..at])?;
    let rest = &def[at + " <> '".len()..];
    let (literal, _) = rest.split_once('\'')?;
    let needs = def.find(" IS NOT NULL")?;
    let needed = ident_before(&def[..needs])?;
    Some((column, literal, needed))
}

/// `location_id IS NOT NULL OR requires_shipping = false` — the column that is
/// only required sometimes, and the flag that excuses it.
fn excused_by_flag(def: &str) -> Option<(&str, &str)> {
    let at = def.find(" = false")?;
    let flag = ident_before(&def[..at])?;
    let needs = def.find(" IS NOT NULL")?;
    let needed = ident_before(&def[..needs])?;
    Some((needed, flag))
}

/// The pair in `period_end > period_start` (or `>=`): the column that has to
/// come later, and the one it is measured against. A period whose ends are
/// equal is not a period, and a seeder writing `now()` into both would make
/// that true of every row.
fn ordered_pair(def: &str) -> Option<(&str, &str)> {
    for op in [" >= ", " > "] {
        if let Some(at) = def.find(op) {
            let later = ident_before(&def[..at])?;
            let rest = &def[at + op.len()..];
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let earlier = &rest[..end];
            if !earlier.is_empty() {
                return Some((later, earlier));
            }
        }
    }
    None
}

/// The last identifier in a fragment, past whatever parentheses surround it.
fn ident_before(fragment: &str) -> Option<&str> {
    let trimmed = fragment.trim_end_matches([')', ' ']);
    let start = trimmed
        .rfind(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map_or(0, |at| at + 1);
    (start < trimmed.len()).then(|| &trimmed[start..])
}

/// A value a check constraint says is allowed, where it says so plainly.
///
/// Postgres renders `in ('a', 'b')` as `= ANY (ARRAY['a'::text, ...])`, which
/// is the shape the enum-as-text convention leaves behind. A definition using
/// a regular expression is no help and is left alone.
fn accepted_literal(def: &str) -> Option<String> {
    if def.contains('~') {
        return None;
    }
    let rest = match def.split_once("ARRAY['") {
        Some((_, rest)) => rest,
        None => def.split_once(" = '")?.1,
    };
    let (literal, _) = rest.split_once('\'')?;
    (!literal.is_empty()).then(|| literal.to_string())
}

/// Whether a constraint definition talks about this column and not one whose
/// name merely contains it.
fn mentions(def: &str, column: &str) -> bool {
    def.match_indices(column).any(|(at, _)| {
        let before = def[..at].chars().next_back();
        let after = def[at + column.len()..].chars().next();
        let boundary = |c: Option<char>| !matches!(c, Some(c) if c.is_alphanumeric() || c == '_');
        boundary(before) && boundary(after)
    })
}

/// What to write into a column, or why nothing can be.
fn value(
    table: &Table,
    column: &Column,
    id: Uuid,
    seeded: &HashMap<String, Uuid>,
) -> Result<String, String> {
    if let Some(parent) = table.parents.get(&column.name) {
        // A row pointing at itself satisfies its own foreign key.
        let target = if parent == &table.name {
            Some(&id)
        } else {
            seeded.get(parent)
        };
        return match target {
            Some(parent_id) => Ok(format!("'{parent_id}'::uuid")),
            None => Err(format!("no row was seeded in {parent}")),
        };
    }

    if let Some(literal) = table.literals.get(&column.name) {
        return Ok(format!("'{}'", literal.replace('\'', "''")));
    }

    Ok(match column.data_type.as_str() {
        "uuid" => format!("'{}'::uuid", Uuid::now_v7()),
        "boolean" => "false".into(),
        "integer" | "bigint" | "smallint" | "numeric" | "real" | "double precision" => "1".into(),
        "json" | "jsonb" => format!("'{{}}'::{}", column.data_type),
        "timestamp with time zone" | "date" | "time with time zone" => "now()".into(),
        "text" | "character varying" | "character" => match column.length {
            Some(2) => "'US'".into(),
            Some(3) => "'USD'".into(),
            _ => "'tezgah'".into(),
        },
        "ARRAY" => "'{}'".into(),
        other => return Err(format!("no value is known for a {other} column")),
    })
}

/// Bends the columns already chosen until the checks that tie them together
/// are satisfied: an array that may not be empty, an array that must contain
/// another column, a column that is only required because of what a second one
/// holds.
fn satisfy_checks(
    table: &Table,
    chosen: &mut Vec<(String, String)>,
    id: Uuid,
    seeded: &HashMap<String, Uuid>,
) -> Result<(), String> {
    fn held(chosen: &[(String, String)], name: &str) -> Option<String> {
        chosen
            .iter()
            .find(|(column, _)| column == name)
            .map(|(_, literal)| literal.clone())
    }

    fn put(chosen: &mut Vec<(String, String)>, name: &str, literal: String) {
        chosen.retain(|(had, _)| had != name);
        chosen.push((name.to_string(), literal));
    }

    for def in &table.checks {
        if let Some(name) = non_empty_array(def) {
            let column = table.columns.iter().find(|c| c.name == name);
            if let (Some(column), Some(_)) = (column, held(chosen, name)) {
                put(
                    chosen,
                    name,
                    format!("array['tezgah']::{}[]", column.element),
                );
            }
        }

        if let Some((scalar, array)) = membership(def) {
            let column = table.columns.iter().find(|c| c.name == array);
            if let (Some(column), Some(inside)) = (column, held(chosen, scalar)) {
                put(
                    chosen,
                    array,
                    format!("array[{inside}]::{}[]", column.element),
                );
            }
        }

        if let Some((needed, flag)) = excused_by_flag(def) {
            if held(chosen, needed).is_none() && table.columns.iter().any(|c| c.name == flag) {
                put(chosen, flag, "false".into());
            }
        }

        if let Some((later, earlier)) = ordered_pair(def) {
            let both_timestamps = |name: &str| {
                table
                    .columns
                    .iter()
                    .any(|c| c.name == name && c.data_type == "timestamp with time zone")
            };
            if both_timestamps(later) && both_timestamps(earlier) && held(chosen, earlier).is_some()
            {
                put(chosen, later, "now() + interval '1 day'".into());
            }
        }

        if let Some((name, literal, needed)) = implication(def) {
            let triggered = held(chosen, name).as_deref() == Some(format!("'{literal}'").as_str());
            let column = table.columns.iter().find(|c| c.name == needed);
            if let (true, None, Some(column)) = (triggered, held(chosen, needed), column) {
                let filled = value(table, column, id, seeded)?;
                chosen.push((needed.to_string(), filled));
            }
        }
    }

    Ok(())
}

/// Seeds one row per table in `shop.here`, parents before children.
///
/// Returns the tables that got a row, and the ones that did not with the reason.
async fn seed(shop: &Shop) -> (Vec<String>, Vec<(String, String)>) {
    let tables = describe(shop).await;

    let mut seeded: HashMap<String, Uuid> = HashMap::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut pending: Vec<&Table> = tables.iter().collect();

    loop {
        let mut progressed = false;
        let mut next = Vec::new();

        for table in pending {
            let waiting = table.parents.iter().any(|(column, parent)| {
                parent != &table.name
                    && !seeded.contains_key(parent)
                    && !skipped.iter().any(|(name, _)| name == parent)
                    && table
                        .columns
                        .iter()
                        .any(|c| &c.name == column && c.required)
            });
            if waiting {
                next.push(table);
                continue;
            }

            let id = Uuid::now_v7();
            let mut chosen: Vec<(String, String)> = Vec::new();
            let mut refused = None;

            for column in &table.columns {
                if !column.required || column.name == "id" || column.name == "scope" {
                    continue;
                }
                match value(table, column, id, &seeded) {
                    Ok(literal) => chosen.push((column.name.clone(), literal)),
                    Err(why) => {
                        refused = Some(format!("{}: {why}", column.name));
                        break;
                    }
                }
            }

            if let Some(why) = refused {
                skipped.push((table.name.clone(), why));
                progressed = true;
                continue;
            }

            if let Err(why) = satisfy_checks(table, &mut chosen, id, &seeded) {
                skipped.push((table.name.clone(), why));
                progressed = true;
                continue;
            }

            let mut names = vec![quote("id"), quote("scope")];
            let mut values = vec![format!("'{id}'::uuid"), format!("'{}'::uuid", shop.here.0)];
            for (column, literal) in &chosen {
                names.push(quote(column));
                values.push(literal.clone());
            }

            let statement = format!(
                "insert into {} ({}) values ({})",
                quote(&table.name),
                names.join(", "),
                values.join(", ")
            );

            let mut tx = shop.begin().await;
            let written = sqlx::query(&statement).execute(&mut *tx).await;
            match written {
                Ok(_) => {
                    tx.commit().await.expect("to commit the seed row");
                    seeded.insert(table.name.clone(), id);
                }
                Err(error) => {
                    let _ = tx.rollback().await;
                    skipped.push((table.name.clone(), error.to_string()));
                }
            }
            progressed = true;
        }

        if next.is_empty() {
            break;
        }
        if !progressed {
            for table in next {
                skipped.push((
                    table.name.clone(),
                    "circular required foreign keys, so no row can be first".into(),
                ));
            }
            break;
        }
        pending = next;
    }

    let mut covered: Vec<String> = seeded.into_keys().collect();
    covered.sort();
    skipped.sort();
    (covered, skipped)
}

#[tokio::test]
async fn no_registered_table_shows_its_rows_to_another_scope() {
    let shop = Shop::open().await;
    let (covered, skipped) = seed(&shop).await;

    assert!(
        !covered.is_empty(),
        "no table could be seeded, so this test proved nothing"
    );

    let mut leaks: Vec<String> = Vec::new();
    let mut theirs = shop.begin_as(shop.elsewhere).await;

    for table in &covered {
        let quoted = quote(table);

        let seen: i64 = sqlx::query_scalar(&format!("select count(*) from {quoted}"))
            .fetch_one(&mut *theirs)
            .await
            .unwrap_or_else(|e| panic!("{table}: another scope could not even count: {e}"));
        if seen != 0 {
            leaks.push(format!(
                "{table}: another scope can read {seen} row(s) of it"
            ));
        }

        let changed = sqlx::query(&format!("update {quoted} set updated_at = updated_at"))
            .execute(&mut *theirs)
            .await
            .unwrap_or_else(|e| panic!("{table}: the update was rejected outright: {e}"))
            .rows_affected();
        if changed != 0 {
            leaks.push(format!(
                "{table}: another scope updated {changed} row(s) of it"
            ));
        }

        let removed = sqlx::query(&format!("delete from {quoted}"))
            .execute(&mut *theirs)
            .await
            .unwrap_or_else(|e| panic!("{table}: the delete was rejected outright: {e}"))
            .rows_affected();
        if removed != 0 {
            leaks.push(format!(
                "{table}: another scope deleted {removed} row(s) of it"
            ));
        }
    }

    theirs.rollback().await.expect("to roll back");
    shop.close().await;

    assert!(
        leaks.is_empty(),
        "{} of {} tables leak across scopes:\n  {}\n(not covered by this run: {:?})",
        leaks.len(),
        covered.len(),
        leaks.join("\n  "),
        skipped.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );
}

/// Tables the seeder cannot build a row for, so isolation would be asserted for
/// everything else and these named rather than quietly missing.
///
/// Empty, and it may only shrink: a table that starts seeding fails this test
/// until it is taken off, and one that stops has to be understood rather than
/// listed.
const NOT_YET_SEEDED: &[&str] = &[];

#[tokio::test]
async fn every_registered_table_can_be_seeded_so_isolation_is_actually_asked() {
    let shop = Shop::open().await;
    let (covered, skipped) = seed(&shop).await;
    shop.close().await;

    let skipped: Vec<_> = skipped
        .into_iter()
        .filter(|(_, why)| !why.contains("no row was seeded in"))
        .collect();

    let fresh: Vec<_> = skipped
        .iter()
        .filter(|(name, _)| !NOT_YET_SEEDED.contains(&name.as_str()))
        .map(|(name, why)| format!("{name}: {why}"))
        .collect();

    assert!(
        fresh.is_empty(),
        "{} of {} tables could not be given a row, so nothing proves they are isolated:\n  {}",
        fresh.len(),
        covered.len() + skipped.len(),
        fresh.join("\n  ")
    );

    let mended: Vec<_> = NOT_YET_SEEDED
        .iter()
        .filter(|name| !skipped.iter().any(|(had, _)| had == *name))
        .collect();

    assert!(
        mended.is_empty(),
        "these can be seeded now and should come off NOT_YET_SEEDED: {mended:?}"
    );
}

/// Reading somebody else's row is refused by the policy. Pointing at it was
/// not: Postgres checks a foreign key with row security bypassed, so the
/// reference was invisible to every read and entirely real to the constraint —
/// and `restrict` and `set null` fired across the boundary.
#[tokio::test]
async fn a_row_cannot_reference_another_scopes_row() {
    let shop = Shop::open().await;

    let theirs = Uuid::now_v7();
    let mut elsewhere = shop.begin_as(shop.elsewhere).await;
    sqlx::query(r#"insert into "order" (id, scope, currency_code) values ($1, $2, 'TRY')"#)
        .bind(theirs)
        .bind(shop.elsewhere.0)
        .execute(&mut *elsewhere)
        .await
        .expect("an order in the other shop");
    elsewhere.commit().await.expect("to commit");

    let mut mine = shop.begin().await;
    let refused = sqlx::query(
        "insert into order_status_history (id, scope, order_id, was, became)
         values ($1, $2, $3, 'pending', 'completed')",
    )
    .bind(Uuid::now_v7())
    .bind(shop.here.0)
    .bind(theirs)
    .execute(&mut *mine)
    .await;
    mine.rollback().await.expect("to give the connection back");

    assert!(
        refused.is_err(),
        "a row in one scope referenced an order in another"
    );

    shop.close().await;
}
