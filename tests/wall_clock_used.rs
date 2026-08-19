//! `Clock` exists so "expires in an hour" is testable without sleeping — the
//! port table in `README.md` says so in those words. Reading the wall clock
//! straight from `chrono` instead of through `ctx.now()` writes a timestamp
//! nobody can move: a host cannot wind it forward to prove a lease expired,
//! or hold it still to prove one is kept alive by a heartbeat.
//!
//! Issue #166 found six such reads and settled the question for each: the
//! five workflow-lease sites (`src/workflow.rs`) now take `ctx.now()` because
//! `Ctx` is already in scope at every one of them and a lease is exactly the
//! shape the port was built for. The `src/tax.rs` placeholder was never a
//! real clock read — its only caller overwrites the field immediately — so it
//! is `Default::default()` now, not a second, unused wall-clock read.
//!
//! This reads `src/`, skips `src/providers/` (real wall-clock time for
//! request signatures, and on its way to kasapay per the README), skips
//! `#[cfg(test)] mod tests { .. }` bodies (fixtures may use whatever clock
//! they like), and fails when a bare `Utc::now()` remains anywhere else.
//!
//! What this proves: no production code path under `src/` reads the wall
//! clock outside a host's own `Clock` implementation. What it does not
//! prove: that every `ctx.now()` call is *correct* — only that it went
//! through the port rather than around it.

use std::path::Path;

/// Production wall-clock reads outside `Clock`, each with the reason it is
/// not a hole. This list may only shrink — issue #166 closed it at zero by
/// moving every production read to `ctx.now()`; the next one found does not
/// get an entry here without that reasoning first, and the reasoning belongs
/// in a comment at the call site as well as this list.
const TOLERATED: [(&str, &str); 0] = [];

const CFG_TEST: &str = "#[cfg(test)]";

/// Blanks out the body of every `#[cfg(test)] mod .. { .. }`, keeping line
/// numbers intact (newlines survive, everything else becomes a space) so a
/// reported location still points at the real line in the real file. Walks
/// forward with `str::find` rather than re-scanning from byte zero each
/// step, so a whole file costs one pass rather than one per character.
fn strip_test_mods(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;

    while let Some(hit) = rest.find(CFG_TEST) {
        out.push_str(&rest[..hit]);
        let after_attr = &rest[hit + CFG_TEST.len()..];

        let Some(brace_at) = after_attr.find('{') else {
            // No brace follows — not a `mod { .. }` this test understands.
            // Leave the rest as-is rather than guess.
            out.push_str(&rest[hit..]);
            return out;
        };
        out.push_str(&rest[hit..hit + CFG_TEST.len() + brace_at]);

        let body = &after_attr[brace_at..];
        let mut depth = 0i32;
        let mut end = body.len();
        for (idx, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                end = idx + ch.len_utf8();
                break;
            }
        }
        // Blank the body, newlines kept, so a reported line number after this
        // point still points at the real line in the real file.
        for ch in body[..end].chars() {
            out.push(if ch == '\n' { '\n' } else { ' ' });
        }
        rest = &body[end..];
    }

    out.push_str(rest);
    out
}

/// Every `.rs` under `root`, skipping `providers/`, as `(display path, text)`.
fn collect(root: &Path, dir: &Path, into: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("src/ to be readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "providers") {
                continue;
            }
            collect(root, &path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let display = format!(
                "src/{}",
                path.strip_prefix(root)
                    .expect("a path under src/")
                    .to_string_lossy()
            );
            into.push((
                display,
                std::fs::read_to_string(&path).expect("a source file"),
            ));
        }
    }
}

/// `"path:line"` for every `Utc::now()` call left in `source` after test
/// bodies are stripped. Matches `Utc::now()` and `chrono::Utc::now()` alike,
/// since the second ends with the first.
fn wall_clock_reads(path: &str, source: &str) -> Vec<String> {
    let production = strip_test_mods(source);
    let mut hits = Vec::new();

    for (idx, line) in production.lines().enumerate() {
        if line.contains("Utc::now()") {
            hits.push(format!("{path}:{}", idx + 1));
        }
    }

    hits
}

#[test]
fn no_production_code_under_src_reads_the_wall_clock_outside_a_provider() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut sources = Vec::new();
    collect(&dir, &dir, &mut sources);
    sources.sort();
    assert!(!sources.is_empty(), "no source files were scanned");

    let mut found = Vec::new();
    for (path, source) in &sources {
        found.extend(wall_clock_reads(path, source));
    }

    let mut silent = Vec::new();
    let mut used = Vec::new();
    for at in &found {
        match TOLERATED.iter().find(|(loc, _)| *loc == at.as_str()) {
            Some((loc, _)) => used.push(*loc),
            None => silent.push(at.clone()),
        }
    }

    assert!(
        silent.is_empty(),
        "these read the wall clock directly instead of through `ctx.now()`:\n  {}\n\
         Take a `&Ctx<'_>` and call `ctx.now()`, or explain in a comment at the \
         call site and in TOLERATED here why the port cannot reach that far.",
        silent.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(loc, _)| *loc)
        .filter(|loc| !used.contains(loc))
        .collect();
    assert!(
        stale.is_empty(),
        "these go through ctx.now() now, or are gone; take them out of TOLERATED: {stale:?}"
    );
}

#[test]
fn a_wall_clock_read_inside_a_test_module_is_not_flagged() {
    let source = "fn production() {}\n\
                  #[cfg(test)]\n\
                  mod tests {\n\
                      fn fixture() -> chrono::DateTime<chrono::Utc> {\n\
                          chrono::Utc::now()\n\
                      }\n\
                  }\n";
    assert!(
        wall_clock_reads("src/example.rs", source).is_empty(),
        "a fixture's own clock is not production code"
    );
}

#[test]
fn a_wall_clock_read_outside_any_test_module_is_flagged() {
    // A fixture string standing in for a new production call site — this is
    // what the latch above must catch tomorrow, not a real file today.
    let source = "async fn lease_something(ctx: &Ctx<'_>) -> DateTime<Utc> {\n\
                      chrono::Utc::now() + LEASE\n\
                  }\n";
    let hits = wall_clock_reads("src/example.rs", source);
    assert_eq!(
        hits,
        vec!["src/example.rs:2".to_string()],
        "a bare Utc::now() outside any #[cfg(test)] module must be reported"
    );
}
