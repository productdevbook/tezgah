//! "No route and no example may call `payment::capture_only` or
//! `payment::refund_only` directly" — the issue's own words for #122.
//!
//! `settlement::capture` and `settlement::refund` are the only correct
//! callers outside `payment.rs` itself: they take the money and then do what
//! must follow, and a caller reaching past them silently skips that. This
//! test greps `src/` for the two names and fails when either turns up
//! anywhere but `src/settlement.rs` or `src/payment.rs`.
//!
//! What it proves: nothing under `src/` — no route, no example, no other
//! domain — calls the low-level function directly. What it does not prove:
//! that a *host*, outside this crate, calls `settlement` rather than reaching
//! past it into `payment` from its own webhook handler; that is a contract
//! the docs state and this crate cannot enforce past its own boundary.
//! `examples/` is scanned too, and is empty at the time of writing — this
//! test still reads it, so a future example is covered the day it is added.
//!
//! A small `TOLERATED` list, shrink-only, is for a legitimate direct caller
//! that is neither `settlement` nor a test.

use std::path::{Path, PathBuf};

/// `(file, function, reason)`. Adding to this is not a fix.
const TOLERATED: [(&str, &str, &str); 0] = [];

fn files(dir: &Path, ext: &str, into: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files(&path, ext, into);
        } else if path.extension().is_some_and(|e| e == ext) {
            into.push(path);
        }
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn offenders(function: &str) -> Vec<String> {
    let mut sources = Vec::new();
    files(&root().join("src"), "rs", &mut sources);
    files(&root().join("examples"), "rs", &mut sources);
    sources.sort();

    let needle = format!("payment::{function}(");
    let mut found = Vec::new();

    for path in &sources {
        if path.ends_with("settlement.rs") || path.ends_with("payment.rs") {
            continue;
        }
        let text = std::fs::read_to_string(path).expect("a source file to be readable");
        if !text.contains(&needle) {
            continue;
        }
        let display = path
            .strip_prefix(root())
            .unwrap_or(path)
            .display()
            .to_string();
        if TOLERATED
            .iter()
            .any(|(file, name, _)| *file == display && *name == function)
        {
            continue;
        }
        found.push(display);
    }

    found
}

#[test]
fn nothing_outside_settlement_calls_the_low_level_capture_or_refund() {
    let mut wrong = offenders("capture_only");
    wrong.extend(offenders("refund_only"));

    assert!(
        wrong.is_empty(),
        "these call payment::capture_only or payment::refund_only directly, \
         bypassing the gift card, digital entitlement and subscription work \
         `settlement` does; call `settlement::capture` / `settlement::refund` \
         instead: {wrong:?}"
    );
}

/// The latch has teeth: a fixture standing in for a wrongly-written route,
/// checked with the same substring test above rather than by adding a real
/// bad file to `src/`.
#[test]
fn the_latch_would_catch_a_direct_call() {
    let bad_route = r#"
        pub async fn capture_payment(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<()> {
            payment::capture_only(tx, ctx, id, amount, None).await?;
            Ok(())
        }
    "#;

    assert!(bad_route.contains("payment::capture_only("));

    let good_route = r#"
        pub async fn capture_payment(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: PaymentId) -> Result<()> {
            settlement::capture(tx, ctx, id, amount, None).await?;
            Ok(())
        }
    "#;

    assert!(!good_route.contains("payment::capture_only("));
}
