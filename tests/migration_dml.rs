//! A migration that writes rows must announce the scope it is writing them in.
//!
//! Migrations run as the table owner, `tezgah_scoped` FORCES row-level security,
//! and nothing sets `app.scope` before a migration runs. So `scope = null`,
//! which is never true, and a plain `update` or `insert` against a scoped table
//! matches no rows at all — silently. 0022 did exactly that: the backfill
//! reported success having touched nothing, and the constraint after it then
//! validated a table it had never corrected. 0024 wrote the shape that works —
//! loop `tezgah_scope`, `set_config('app.scope', ..)`, then do the work — and
//! nothing until now stopped the next migration forgetting it again.
//!
//! Read from `migrations/` rather than from a database, because what is wrong
//! is what the file says, and a database that ran it has already lost the
//! evidence.
//!
//! What this cannot see: DML built with `execute format(..)`, and a scope loop
//! that names the wrong scopes. What it deliberately ignores: the `tezgah_*`
//! bookkeeping tables, which carry no scope and have no policy to fall foul of,
//! and the bodies of functions and procedures, which run later under whatever
//! scope the caller set.

use std::fs;
use std::path::PathBuf;

/// Bare DML that is already in a migration and cannot be edited out of it,
/// migrations being append-only. This list may only shrink.
const TOLERATED: [(&str, &str); 2] = [
    ("0017_workflow_parallel.sql", "workflow_step"),
    ("0022_order_operator_status.sql", "order_return"),
];

/// A verb and what it was pointed at.
#[derive(Debug, PartialEq, Eq)]
struct Bare {
    verb: String,
    table: String,
}

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

/// The stretches of a migration whose statements the database executes under
/// no scope: everything outside a dollar-quoted body, plus the body of any
/// `do` block that does not announce a scope. A function or procedure body is
/// not one of these — it is called later, by code that has set one.
fn unscoped_regions(sql: &str) -> Vec<String> {
    let text = without_comments(sql).to_lowercase();
    let marks = dollar_marks(&text);

    let mut top = String::new();
    let mut regions = Vec::new();
    let mut cursor = 0;
    let mut pair = 0;

    while pair + 1 < marks.len() {
        let open = marks[pair];
        let close = marks[pair + 1];
        let intro = &text[cursor..open];
        let body = &text[open + 2..close];

        let is_do = intro.trim_end().ends_with("do")
            && intro
                .trim_end()
                .strip_suffix("do")
                .and_then(|rest| rest.chars().last())
                .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let announces = body.contains("tezgah_scope") && body.contains("set_config('app.scope'");

        if is_do && !announces {
            regions.push(body.to_owned());
        }

        top.push_str(intro);
        top.push(';');
        cursor = close + 2;
        pair += 2;
    }

    top.push_str(&text[cursor..]);
    regions.push(top);
    regions
}

fn word_ends_at(text: &str, word: &str) -> bool {
    text.strip_suffix(word)
        .and_then(|rest| rest.chars().last())
        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
}

/// Whether what comes before this point can only be the end of the statement
/// before it — so `for update`, `on update` and `before update of` are not
/// mistaken for a write.
fn at_a_statement_start(region: &str, at: usize) -> bool {
    let before = region[..at].trim_end();
    if before.is_empty() || before.ends_with(';') {
        return true;
    }
    ["loop", "begin", "then", "else"]
        .iter()
        .any(|kw| before.ends_with(kw) && word_ends_at(before, kw))
}

fn table_after(region: &str, from: usize) -> Option<String> {
    let rest = region[from..].trim_start();
    let mut chars = rest.chars();
    match chars.next()? {
        '"' => {
            let end = rest[1..].find('"')?;
            Some(rest[1..1 + end].to_owned())
        }
        c if c.is_alphabetic() || c == '_' => {
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            Some(rest[..end].to_owned())
        }
        _ => None,
    }
}

/// Every write a migration makes with no scope announced, the `tezgah_*`
/// registry aside.
fn bare_dml(sql: &str) -> Vec<Bare> {
    let mut found = Vec::new();

    for region in unscoped_regions(sql) {
        for (verb, keyword) in [("insert", "insert"), ("update", "update")] {
            let mut from = 0;
            while let Some(at) = region[from..].find(keyword) {
                let at = from + at;
                from = at + keyword.len();

                if !at_a_statement_start(&region, at) {
                    continue;
                }

                let mut after = &region[at + keyword.len()..];
                if verb == "insert" {
                    let trimmed = after.trim_start();
                    let Some(rest) = trimmed.strip_prefix("into") else {
                        continue;
                    };
                    after = rest;
                } else if !after.starts_with(|c: char| c.is_whitespace()) {
                    continue;
                }

                let offset = region.len() - after.len();
                let Some(table) = table_after(&region, offset) else {
                    continue;
                };
                if table.starts_with("tezgah_") {
                    continue;
                }

                found.push(Bare {
                    verb: verb.to_owned(),
                    table,
                });
            }
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
fn no_migration_writes_rows_without_announcing_a_scope() {
    let files = migrations();
    assert!(!files.is_empty(), "no migrations were scanned");

    let mut offences = Vec::new();
    for (name, sql) in &files {
        for bare in bare_dml(sql) {
            if TOLERATED
                .iter()
                .any(|(file, table)| file == name && *table == bare.table)
            {
                continue;
            }
            offences.push(format!("{name}: {} {}", bare.verb, bare.table));
        }
    }

    assert!(
        offences.is_empty(),
        "these run under forced row-level security with no `app.scope` set, so they \
         match no rows and say nothing about it; loop `tezgah_scope` with \
         `set_config('app.scope', ..)` around them as 0024 does: {offences:?}"
    );
}

#[test]
fn the_lint_catches_a_backfill_that_forgot_the_scope() {
    let wrong = "\
set lock_timeout = '3s';

alter table \"order\" add column reference text;

update \"order\" set reference = display_id::text where reference is null;

alter table \"order\" alter column reference set not null;
";

    assert_eq!(
        bare_dml(wrong),
        vec![Bare {
            verb: "update".to_owned(),
            table: "order".to_owned(),
        }]
    );
}

#[test]
fn the_lint_passes_a_backfill_that_announced_the_scope() {
    let right = "\
alter table \"order\" add column reference text;

do $$
declare
    s uuid;
begin
    for s in select id from tezgah_scope loop
        perform set_config('app.scope', s::text, true);

        update \"order\" set reference = display_id::text where reference is null;
    end loop;

    perform set_config('app.scope', '', true);
end
$$;
";

    assert_eq!(bare_dml(right), Vec::new());
}

/// A function's body runs later, under whatever scope its caller set, so DML in
/// one is not what this is looking for.
#[test]
fn the_lint_leaves_a_function_body_alone() {
    let trigger = "\
create or replace function tezgah_touch_order() returns trigger
language plpgsql as $$
begin
    update \"order\" set updated_at = now() where scope = new.scope and id = new.order_id;
    return new;
end
$$;
";

    assert_eq!(bare_dml(trigger), Vec::new());
}

/// `for update`, `on update` and `before update of` are not writes.
#[test]
fn the_lint_does_not_mistake_the_word_update_for_a_write() {
    let innocent = "\
create trigger order_touch before update of status on \"order\"
    for each row execute function tezgah_touch();

create table thing (
    id uuid primary key,
    other_id uuid references other (id) on update cascade
);
";

    assert_eq!(bare_dml(innocent), Vec::new());
}
