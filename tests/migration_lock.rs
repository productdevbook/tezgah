//! A migration that adds an index to a table already carrying rows takes that
//! table's lock for the duration of the build — and #172 measured that this
//! crate does not have to: sqlx 0.8 honours a literal `-- no-transaction`
//! first line by running the migration outside a transaction, which is what
//! `create index concurrently` requires (Postgres refuses it inside one).
//!
//! A migration that also creates the table it indexes needs none of this — a
//! table with no rows costs nothing to lock — so this only looks at an index
//! on a table that already existed when the migration ran.
//!
//! Read from `migrations/` rather than from a database, for the same reason
//! `tests/migration_dml.rs` does: what is wrong is what the file says.
//!
//! What this cannot see: an index built with `execute format(..)` inside a
//! `do` block, where the table name is a `%I` placeholder rather than a
//! literal — 0031 does this for four tables, dynamically, and separately adds
//! two more with a literal top-level `create index` that this test does see.

use std::fs;
use std::path::PathBuf;

/// (migration, table) pairs that indexed an existing table before this rule
/// existed — measured by running this same scan against `migrations/` before
/// the rule was written, not assumed. This list may only shrink: a migration
/// written after #172 landed has no excuse to add to it.
const TOLERATED: &[(&str, &str)] = &[
    ("0017_workflow_parallel.sql", "workflow_step"),
    ("0023_payment_collection_cart.sql", "payment_collection"),
    ("0026_scoped_foreign_keys.sql", "order_transfer"),
    ("0028_installment_and_invoice.sql", "payment_collection"),
    ("0029_agreement_and_withdrawal.sql", "order_return"),
    ("0030_scoped_catalogue_keys.sql", "product_option_value"),
    (
        "0030_scoped_catalogue_keys.sql",
        "product_variant_option_value",
    ),
    ("0031_tax_identity.sql", "product"),
    ("0031_tax_identity.sql", "product_variant"),
    ("0031_tax_identity.sql", "cart_line_item_tax_line"),
    ("0031_tax_identity.sql", "cart_shipping_method_tax_line"),
    ("0031_tax_identity.sql", "order_line_item_tax_line"),
    ("0031_tax_identity.sql", "order_shipping_method_tax_line"),
    ("0039_variant_giftcard_and_change_actions.sql", "gift_card"),
    ("0041_scoped_keys_remainder.sql", "order_invoice"),
    ("0042_subscription.sql", "cart_line_item"),
    ("0042_subscription.sql", "order_line_item"),
    ("0046_order_subscription_link.sql", "order"),
    ("0056_cart_line_item_bundle_key.sql", "cart_line_item"),
    ("0059_subscription_prepaid.sql", "subscription_order"),
    ("0060_cart_line_item_plan_key.sql", "cart_line_item"),
    ("0071_payout_line_capture_attribution.sql", "payout_line"),
    ("0075_reservation_line_item_scope.sql", "reservation_item"),
];

fn without_comments(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    for line in sql.lines() {
        match line.find("--") {
            Some(at) => out.push_str(&line[..at]),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

fn dollar_marks(sql: &str) -> Vec<usize> {
    let bytes = sql.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'$' && bytes[i + 1] == b'$' {
            found.push(i);
            i += 2;
        } else {
            i += 1;
        }
    }
    found
}

/// The migration's text with every dollar-quoted body cut out — a function or
/// `do` block runs later, or under whatever the block itself sets up, so its
/// statements are not the top-level DDL this test is looking for.
fn top_level(sql: &str) -> String {
    let text = without_comments(sql).to_lowercase();
    let marks = dollar_marks(&text);

    let mut top = String::new();
    let mut cursor = 0;
    let mut pair = 0;
    while pair + 1 < marks.len() {
        top.push_str(&text[cursor..marks[pair]]);
        cursor = marks[pair + 1] + 2;
        pair += 2;
    }
    top.push_str(&text[cursor..]);
    top
}

/// A statement's words, punctuation and quotes stripped — table and column
/// names in this codebase are always plain identifiers, so this is simpler
/// than a real SQL tokenizer and exact for what it reads.
fn tokens(statement: &str) -> Vec<String> {
    statement
        .replace(['(', ')', ','], " ")
        .split_whitespace()
        .map(|t| t.trim_matches('"').to_owned())
        .collect()
}

fn at(tokens: &[String], i: usize, word: &str) -> bool {
    tokens.get(i).is_some_and(|t| t == word)
}

/// The table a `create table [if not exists] <name>` statement makes, or
/// `None` if this statement is not one.
fn table_created_by(tokens: &[String]) -> Option<String> {
    if !at(tokens, 0, "create") || !at(tokens, 1, "table") {
        return None;
    }
    let mut i = 2;
    if at(tokens, i, "if") && at(tokens, i + 1, "not") && at(tokens, i + 2, "exists") {
        i += 3;
    }
    tokens.get(i).cloned()
}

/// What a `create [unique] index [concurrently] [if not exists] <name> on
/// [only] <table>` statement names and whether it already runs concurrently.
#[derive(Debug)]
struct IndexStmt {
    table: String,
    concurrent: bool,
}

fn index_created_by(tokens: &[String]) -> Option<IndexStmt> {
    if !at(tokens, 0, "create") {
        return None;
    }
    let mut i = 1;
    if at(tokens, i, "unique") {
        i += 1;
    }
    if !at(tokens, i, "index") {
        return None;
    }
    i += 1;

    let concurrent = at(tokens, i, "concurrently");
    if concurrent {
        i += 1;
    }
    if at(tokens, i, "if") && at(tokens, i + 1, "not") && at(tokens, i + 2, "exists") {
        i += 3;
    }

    i += 1; // the index's own name
    if !at(tokens, i, "on") {
        return None;
    }
    i += 1;
    if at(tokens, i, "only") {
        i += 1;
    }

    tokens
        .get(i)
        .cloned()
        .map(|table| IndexStmt { table, concurrent })
}

/// Every `(table, already_concurrent)` this migration's text adds an index to
/// where the table is not one the same migration also creates.
fn indexes_on_existing_tables(sql: &str) -> Vec<(String, bool)> {
    let top = top_level(sql);

    let mut created: Vec<String> = Vec::new();
    let mut found = Vec::new();

    for statement in top.split(';') {
        let words = tokens(statement);
        if let Some(table) = table_created_by(&words) {
            created.push(table);
            continue;
        }
        if let Some(stmt) = index_created_by(&words)
            && !created.contains(&stmt.table)
        {
            found.push((stmt.table, stmt.concurrent));
        }
    }

    found
}

fn migrations() -> Vec<(String, String)> {
    let mut files: Vec<PathBuf> = fs::read_dir("migrations")
        .expect("migrations to be readable")
        .map(|entry| entry.expect("a directory entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();

    files
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a file name")
                .to_owned();
            let sql = fs::read_to_string(&path).expect("a migration to be readable");
            (name, sql)
        })
        .collect()
}

#[test]
fn a_new_index_on_an_existing_table_opts_out_of_the_transaction_and_runs_concurrently() {
    let files = migrations();
    assert!(!files.is_empty(), "no migrations were scanned");

    let mut offences = Vec::new();
    for (name, sql) in &files {
        let no_transaction = sql.starts_with("-- no-transaction");
        for (table, concurrent) in indexes_on_existing_tables(sql) {
            if TOLERATED
                .iter()
                .any(|(file, tolerated_table)| file == name && *tolerated_table == table)
            {
                continue;
            }
            if !no_transaction || !concurrent {
                offences.push(format!("{name}: index on {table}"));
            }
        }
    }

    assert!(
        offences.is_empty(),
        "these add an index to a table that already existed without both a \
         leading `-- no-transaction` and `concurrently` on the index itself, so \
         the migration takes that table's lock for the build's whole duration: \
         {offences:?}"
    );
}

#[test]
fn every_tolerated_pair_is_still_something_the_scan_finds() {
    // The list is a record of what was true when #172 measured it, not a
    // blanket exemption — if a migration stopped matching (renamed, rewritten)
    // the entry would be dead weight the next reader trusts by accident.
    let files = migrations();
    let by_name: std::collections::HashMap<&str, &str> = files
        .iter()
        .map(|(name, sql)| (name.as_str(), sql.as_str()))
        .collect();

    for (file, table) in TOLERATED.iter() {
        let sql = by_name
            .get(file)
            .unwrap_or_else(|| panic!("{file} cited by TOLERATED is not in migrations/"));
        let found = indexes_on_existing_tables(sql)
            .iter()
            .any(|(found_table, _)| found_table == table);
        assert!(
            found,
            "{file}: TOLERATED cites {table}, but the scan no longer finds an \
             index added to it there"
        );
    }
}

#[test]
fn the_lint_flags_an_index_on_a_table_that_already_existed() {
    let wrong = "\
create index cart_line_item_variant_idx
    on cart_line_item (scope, variant_id);
";
    assert_eq!(
        indexes_on_existing_tables(wrong),
        vec![("cart_line_item".to_owned(), false)]
    );
}

#[test]
fn the_lint_passes_an_index_on_a_table_the_same_migration_creates() {
    let right = "\
create table cart_line_item (
    id uuid primary key,
    variant_id uuid
);

create index cart_line_item_variant_idx
    on cart_line_item (scope, variant_id);
";
    assert_eq!(indexes_on_existing_tables(right), Vec::new());
}

#[test]
fn the_lint_accepts_a_concurrent_index_with_no_transaction() {
    let right = "-- no-transaction
create index concurrently if not exists cart_line_item_variant_idx
    on cart_line_item (scope, variant_id);
";
    assert_eq!(
        indexes_on_existing_tables(right),
        vec![("cart_line_item".to_owned(), true)]
    );
}

#[test]
fn a_quoted_table_name_is_read_the_same_as_a_bare_one() {
    let sql = "\
create unique index \"order_reference_key\"
    on \"order\" (scope, reference);
";
    assert_eq!(
        indexes_on_existing_tables(sql),
        vec![("order".to_owned(), false)]
    );
}
