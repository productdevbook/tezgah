//! "A pair that is supposed to be mirrors writes the same trail."
//!
//! #150, #153, #154 and #155 were the same mistake four times: a spend/give-back
//! or join/leave pair where the production-dominant half wrote no `ctx.audit(..)`
//! while its mirror did. Each was found by reading the pair side by side, never
//! by a test — this is that test, for the pairs known so far.
//!
//! What it proves: a named pair keeps writing (or keeps not writing) an audit
//! row on both sides. What it does not prove: that every mirror pair in the
//! crate is named here — finding a new one is still a reading exercise, this
//! only stops a known one from regressing, and gives a place to add the next
//! one once it is found.
//!
//! `PAIRS` names both sides; `TOLERATED` is for a pair that turns out, on
//! reading, to be legitimately asymmetric — it may only shrink.

use std::path::Path;

/// A named module::function pair that is supposed to leave the same kind of
/// trail on both sides.
const PAIRS: [(&str, &str, &str); 5] = [
    ("credit", "redeem_gift_card", "restore_gift_card"),
    ("credit", "redeem_store_credit", "restore_store_credit"),
    ("inventory", "fulfil_units", "unfulfil_units"),
    ("customer", "join_group", "leave_group"),
    // Neither side writes `ctx.audit(..)` — both emit an event instead, and
    // that agreement is what this test checks. Named here so the pair is
    // watched, not because it needs tolerating.
    ("promotion", "claim", "release"),
];

/// A pair found, on reading, to be asymmetric on purpose. Empty right now:
/// every named pair above agrees. An entry here needs the reason a reader
/// would otherwise ask for.
const TOLERATED: [(&str, &str, &str); 0] = [];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every `fn <name>` in `source` with its body, keyed by name. Good enough
/// for this test's purpose: a name appearing twice in one module (an
/// overload by arity does not happen in this crate) is not distinguished.
fn bodies(source: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;

    while at + 3 < chars.len() {
        if !(chars[at] == 'f'
            && chars[at + 1] == 'n'
            && chars[at + 2].is_whitespace()
            && (at == 0 || !ident_char(chars[at - 1])))
        {
            at += 1;
            continue;
        }

        let mut name_at = at + 3;
        while name_at < chars.len() && chars[name_at].is_whitespace() {
            name_at += 1;
        }
        let name: String = chars[name_at..]
            .iter()
            .take_while(|c| ident_char(**c))
            .collect();
        if name.is_empty() {
            at += 1;
            continue;
        }

        let mut depth = 0i32;
        let mut i = name_at + name.chars().count();
        let mut body_start = None;
        while i < chars.len() {
            match chars[i] {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => depth -= 1,
                '{' if depth <= 0 => {
                    body_start = Some(i);
                    break;
                }
                ';' if depth <= 0 => break,
                _ => {}
            }
            i += 1;
        }
        let Some(body_start) = body_start else {
            at += 1;
            continue;
        };

        let mut depth = 0i32;
        let mut j = body_start;
        while j < chars.len() {
            match chars[j] {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }

        found.push((name, chars[body_start..j.min(chars.len())].iter().collect()));
        at = body_start;
    }

    found
}

fn audits(body: &str) -> bool {
    body.contains("ctx.audit(")
}

fn find<'a>(functions: &'a [(String, String)], name: &str) -> Option<&'a str> {
    functions
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, b)| b.as_str())
}

#[test]
fn mirror_pairs_agree_on_writing_an_audit_row() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut mismatched = Vec::new();
    let mut checked = std::collections::HashSet::new();

    for (module, a, b) in PAIRS {
        let path = src.join(format!("{module}.rs"));
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} to be readable", path.display()));
        let functions = bodies(&source);

        let body_a = find(&functions, a).unwrap_or_else(|| panic!("{module}::{a} not found"));
        let body_b = find(&functions, b).unwrap_or_else(|| panic!("{module}::{b} not found"));

        let key = format!("{module}::{a}/{b}");
        checked.insert(key.clone());

        if TOLERATED
            .iter()
            .any(|(m, x, y)| *m == module && *x == a && *y == b)
        {
            continue;
        }

        if audits(body_a) != audits(body_b) {
            mismatched.push(format!(
                "{key} — {a} {} an audit row, {b} {}",
                if audits(body_a) {
                    "writes"
                } else {
                    "does not write"
                },
                if audits(body_b) {
                    "writes one too"
                } else {
                    "does not"
                },
            ));
        }
    }

    assert!(
        mismatched.is_empty(),
        "these mirror pairs disagree about writing `ctx.audit(..)`:\n  {}\n\
         Either both sides write one, matching entity/action/summary shape, \
         or the asymmetry is real and belongs in TOLERATED with a reason.",
        mismatched.join("\n  ")
    );

    let stale: Vec<&(&str, &str, &str)> = TOLERATED
        .iter()
        .filter(|(m, x, y)| !checked.contains(&format!("{m}::{x}/{y}")))
        .collect();
    assert!(
        stale.is_empty(),
        "these TOLERATED entries no longer name a pair in PAIRS: {stale:?}"
    );
}

#[test]
fn a_deliberately_asymmetric_pair_is_caught() {
    let source = "\
        pub async fn spend(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            ctx.audit(tx, AuditEntry { entity: \"thing\", .. }).await?;\n\
            Ok(())\n\
        }\n\
        pub async fn unspend(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            // no ctx.audit call here — this is the bug this test exists to catch\n\
            Ok(())\n\
        }\n";

    let functions = bodies(source);
    let spend = find(&functions, "spend").expect("spend");
    let unspend = find(&functions, "unspend").expect("unspend");

    assert!(audits(spend), "fixture's spend should audit");
    assert!(
        audits(spend) != audits(unspend),
        "a spend/unspend pair where only one side audits should be detected as \
         asymmetric — if this fails, the detector itself is broken"
    );
}
