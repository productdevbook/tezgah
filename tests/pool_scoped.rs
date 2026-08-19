//! "No query runs against a bare `PgPool`" — issue #160.
//!
//! Row-level security fails open into silence: a read with no `app.scope`
//! set does not error, it returns nothing. `checkout`, `subscription` and
//! `workflow` each hold a `PgPool` rather than a caller's transaction — one
//! of their steps runs later, or on its own, so there is no request-scoped
//! transaction to borrow — and each opened its own transaction and announced
//! a scope on it before doing anything else. `subscription.rs` and
//! `workflow.rs` each named that four-line shape `scoped()`, privately, and
//! #160 found the two copies byte-for-byte identical — one copy too many of
//! the function that makes RLS work (#125's own complaint, about a different
//! pair). `checkout.rs` had the same four lines a third time, inlined and
//! unnamed. All three now call `ports::scoped`, and this test is what keeps
//! a fourth private copy — or a plain `pool.begin()` with the `set_config`
//! left out — from being written again.
//!
//! Mechanically that is grepping for `.fetch_*(pool)` / `.execute(pool)` and
//! the `&*pool` reborrow, matched on the literal identifier `pool`. Every
//! function in the crate that takes a `PgPool` names the parameter that, with
//! no exception, so matching the identifier is exact rather than a guess at
//! a variable name.
//!
//! `tests/` is scanned too. The CI round #160 describes was a test reading
//! `workflow_run` through `shop.pool` with no scope set, not a bug in
//! `src/` — `tests/common/mod.rs` gives out a `Shop` whose `begin()` opens a
//! scoped transaction, but nothing stops a test reaching for `shop.pool`
//! directly, and a test that does gets the same silent empty answer. Unlike
//! `src/`, though, a bare `shop.pool` read is sometimes exactly right:
//! `tests/schema.rs` and `tests/isolation.rs` ask Postgres about its own
//! catalogue — `pg_class`, `pg_constraint`, `information_schema.columns` —
//! and about the `tezgah_*` bookkeeping tables that carry no `scope` column
//! and no policy, the same tables `tests/schema.rs`'s own `UNSCOPED` and
//! `tests/migration_dml.rs`'s doc comment already name for the same reason.
//! None of that is row-level security's to guard, so a call reading one of
//! those — or reading no table at all, the way `tests/isolation.rs`'s
//! `matching` asks Postgres to evaluate `select $1 ~ $2` — is exempt by what
//! its query names rather than listed one by one.
//!
//! What is left after that exemption was measured, not assumed: one call
//! site. `tests/schema.rs::a_connection_that_names_no_scope_sees_nothing`
//! reads `workflow_run` — a real, scoped, row-level-secured table — through
//! `shop.pool` on purpose, because proving that read comes back empty is the
//! whole test. It is `TOLERATED` by name, not by table: exempting the table
//! instead would also excuse a genuine leak of the same rows.
//!
//! What this proves: nothing under `src/` or `tests/` sends a query to a
//! `PgPool` without a scope announced on the transaction it runs in first,
//! except the one line that says why and is checked against a real table.
//! What it does not prove: that `scoped()` was handed the *right* `Ctx` — a
//! function that opens a transaction under its caller's scope and then reads
//! a different scope's id is invisible here, which reads call shape, not
//! values. Nor does it see a query built through `format!` and handed a pool
//! under a name other than `pool` or `shop.pool` — matched textually, the
//! same limit `tests/migration_dml.rs` states for its own grep. And the
//! `tests/` exemption is a text search, not a parse: a query that joined a
//! real scoped table to a catalogue one would read one of `UNGUARDED`'s
//! markers and pass uncaught, the way `touches_only_unguarded_tables`'s own
//! doc comment says plainly. No call site does that today.

use std::path::{Path, PathBuf};

/// `(file, function, reason)`. Adding to this is not a fix — see the module
/// doc for what is exempt by the table a query names instead. This list may
/// only shrink.
const TOLERATED: [(&str, &str, &str); 1] = [(
    "tests/schema.rs",
    "a_connection_that_names_no_scope_sees_nothing",
    "reads workflow_run — a real, row-level-secured table — through the bare \
     pool on purpose: an empty answer is the assertion the test makes, not a \
     miss it made",
)];

const METHODS: [&str; 7] = [
    "fetch_one",
    "fetch_all",
    "fetch_optional",
    "fetch_many",
    "fetch",
    "execute",
    "execute_many",
];

/// Tables and views row-level security does not guard: Postgres's own
/// catalogue, and the scope-free `tezgah_*` bookkeeping tables
/// `tests/schema.rs`'s `UNSCOPED` already names. A query naming one of these
/// answers the same through a bare pool as through a scoped transaction.
const UNGUARDED: [&str; 3] = ["pg_", "information_schema", "tezgah_"];

fn files(dir: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            into.push(path);
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A call site that hands a bare pool straight to sqlx: where it starts, and
/// the query text between the nearest `sqlx::query` before it and the call
/// itself — empty if none is found within the file.
struct BareCall {
    at: usize,
    query: String,
}

/// Every `.method(arg)` in `text` whose `arg`, whitespace stripped, is one of
/// the spellings a `PgPool` is passed under in this crate: the bare `pool`
/// parameter every pool-taking function uses, its `&*pool` reborrow, or a
/// test's `shop.pool` field and its reborrow.
fn bare_pool_calls(text: &str) -> Vec<BareCall> {
    let mut found = Vec::new();
    for method in METHODS {
        let needle = format!(".{method}(");
        let mut from = 0;
        while let Some(rel) = text[from..].find(&needle) {
            let at = from + rel;
            let after = at + needle.len();
            from = at + 1;
            let Some(close_rel) = text[after..].find(')') else {
                continue;
            };
            let close = after + close_rel;
            let arg: String = text[after..close]
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            if matches!(
                arg.as_str(),
                "pool" | "&pool" | "&*pool" | "&shop.pool" | "&*shop.pool"
            ) {
                let query = text[..at]
                    .rfind("sqlx::query")
                    .map(|start| text[start..at].to_string())
                    .unwrap_or_default();
                found.push(BareCall { at, query });
            }
        }
    }
    found
}

/// A query that has nothing row-level security would have hidden: either it
/// names no table at all — a pure computation like `select $1 ~ $2` — or it
/// mentions one of `UNGUARDED`'s markers anywhere in its text.
///
/// This is a text search, not a parse: a query that joined a real scoped
/// table to a catalogue one — `select ... from "order" o join pg_class c on
/// ...` — would read `pg_` in its text and pass here uncaught. No call site
/// in this crate does that today; the day one does, it needs a human, not
/// this heuristic, to say which table the read was really asking about.
fn touches_only_unguarded_tables(query: &str) -> bool {
    let lower = query.to_lowercase();
    let names_a_table = ["from ", "join ", "update ", "insert into", "delete from"]
        .iter()
        .any(|kw| lower.contains(kw));
    !names_a_table || UNGUARDED.iter().any(|marker| lower.contains(marker))
}

/// The `fn` (or `async fn`, `pub(crate) fn`, ...) whose signature line is the
/// nearest one above `at` — a plain backward scan rather than depth-tracked
/// parsing, which is exact here because no call site in this crate sits
/// inside a nested `fn`.
fn enclosing_fn(text: &str, at: usize) -> String {
    let mut offset = 0usize;
    let mut lines = Vec::new();
    for line in text.split_inclusive('\n') {
        lines.push((offset, line));
        offset += line.len();
    }
    let idx = lines
        .iter()
        .rposition(|(start, _)| *start <= at)
        .unwrap_or(0);
    for (_, line) in lines[..=idx].iter().rev() {
        let trimmed = line.trim_start();
        let trimmed = trimmed
            .strip_prefix("pub(crate) ")
            .or_else(|| trimmed.strip_prefix("pub "))
            .unwrap_or(trimmed);
        let trimmed = trimmed.strip_prefix("async ").unwrap_or(trimmed);
        if let Some(rest) = trimmed.strip_prefix("fn ") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                return name;
            }
        }
    }
    "<unknown>".to_string()
}

#[test]
fn no_query_runs_against_a_bare_pool() {
    let mut sources = Vec::new();
    files(&root().join("src"), &mut sources);
    files(&root().join("tests"), &mut sources);
    sources.sort();
    assert!(!sources.is_empty(), "no source files were scanned");

    let mut silent = Vec::new();
    let mut used = Vec::new();

    for path in &sources {
        if path.ends_with("pool_scoped.rs") {
            // This file's own fixtures spell out the bad shape as a string,
            // for `a_bare_pool_query_is_caught` to check the matcher against —
            // not a real call site for the matcher to check.
            continue;
        }
        let text = std::fs::read_to_string(path).expect("a source file to be readable");
        let rel = path
            .strip_prefix(root())
            .expect("a path under the repository")
            .to_string_lossy()
            .replace('\\', "/");
        let under_tests = rel.starts_with("tests/");

        for call in bare_pool_calls(&text) {
            if under_tests && touches_only_unguarded_tables(&call.query) {
                continue;
            }
            let function = enclosing_fn(&text, call.at);
            let line = text[..call.at].matches('\n').count() + 1;

            match TOLERATED
                .iter()
                .find(|(file, func, _)| *file == rel && *func == function)
            {
                Some((file, func, _)) => used.push((*file, *func)),
                None => silent.push(format!("{rel}:{line} — {function}")),
            }
        }
    }

    assert!(
        silent.is_empty(),
        "these read or write through a bare pool, so an unset app.scope answers \
         with silence instead of an error:\n  {}\n\
         Open a transaction through `ports::scoped(pool, ctx)` (or `shop.begin()` \
         in a test) before running the query.",
        silent.join("\n  ")
    );

    let stale: Vec<(&str, &str)> = TOLERATED
        .iter()
        .map(|(file, func, _)| (*file, *func))
        .filter(|entry| !used.contains(entry))
        .collect();
    assert!(
        stale.is_empty(),
        "these read a guarded table now, or are gone; take them out of TOLERATED: {stale:?}"
    );
}

#[test]
fn a_bare_pool_query_is_caught() {
    let text = "async fn leaky(pool: &sqlx::PgPool, ctx: &Ctx<'_>) -> Result<Run> {\n\
                let run = sqlx::query_as(\"select * from workflow_run where scope = $1\")\n    \
                .bind(ctx.scope.0)\n    .fetch_one(pool)\n    .await?;\n\
                Ok(run)\n}\n";
    let calls = bare_pool_calls(text);
    assert_eq!(calls.len(), 1, "a `.fetch_one(pool)` call must be found");
    assert_eq!(enclosing_fn(text, calls[0].at), "leaky");
}

#[test]
fn a_scoped_call_is_not_caught() {
    let text = "async fn honest(pool: &sqlx::PgPool, ctx: &Ctx<'_>) -> Result<Run> {\n\
                let mut tx = scoped(pool, ctx).await?;\n\
                let run = sqlx::query_as(\"select * from workflow_run where scope = $1\")\n    \
                .bind(ctx.scope.0)\n    .fetch_one(&mut *tx)\n    .await?;\n\
                Ok(run)\n}\n";
    assert!(
        bare_pool_calls(text).is_empty(),
        "a query against `&mut *tx` is not a bare-pool call"
    );
}

#[test]
fn a_catalogue_read_through_a_test_pool_is_exempt() {
    let query = "sqlx::query_scalar(\"select t.name from tezgah_table t join pg_class c \
                 on c.relname = t.name\")";
    assert!(
        touches_only_unguarded_tables(query),
        "tezgah_table and pg_class carry no scope and no policy"
    );
}

#[test]
fn a_real_table_read_through_a_test_pool_is_not_exempt() {
    let query = "sqlx::query_scalar(\"select count(*) from workflow_run\")";
    assert!(
        !touches_only_unguarded_tables(query),
        "workflow_run is row-level-secured; a bare-pool read of it needs a \
         TOLERATED entry, not a silent pass"
    );
}
