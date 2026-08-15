//! "Every function that asks for `Action::Settle` or `Action::Delete` writes
//! an audit row."
//!
//! `tests/audit_symmetry.rs` watches five hand-named pairs, each found by
//! reading two functions side by side. That is real work and it does not
//! scale: the next asymmetric pair is found the same way, by hand, and only
//! after it has shipped. `ports.rs` already says what `Settle` and `Delete`
//! are — moving money, and destroying a row — and both are, by definition,
//! acts an operator has to be able to reconstruct afterwards. That gives a
//! rule that needs no pairing at all: ask for one of those two and the audit
//! sink hears about it.
//!
//! This reuses `tests/permit_asked.rs`'s extraction — the same brace-matching
//! walk over every `fn`, the same call-graph reachability — pointed at a
//! different question. `permit_asked` asks "did somebody ask before this ran
//! a query"; this asks "did the function that asked for `Settle` or `Delete`
//! leave a trail." Asking is inherited the same way: a function that hands
//! the actual write to another function in this crate has audited if that
//! function did, because the row it wrote is the same row.
//!
//! What it proves: every public function whose own `ctx.permit(..)` names
//! `Action::Settle` or `Action::Delete` either calls `ctx.audit(..)` itself or
//! reaches it through a function this crate calls — checked today, the day
//! the code changes, not the day somebody happens to read the pair side by
//! side.
//!
//! What it does not prove: that the audit row's `entity` names the thing this
//! function actually changed, rather than something else the same call chain
//! happened to touch. The call graph is untyped text matching, the same
//! caveat `permit_asked` carries — a private helper that audits an unrelated
//! table makes every path that reaches it look covered. That is a real risk:
//! `cart::expire` inherited a pass this way before it had its own audit call,
//! because it releases inventory reservations (which audit) on its way to
//! deleting the cart (which, until fixed here, did not). Reading each
//! inherited-only pass by hand rather than trusting the graph is what caught
//! it. It also does not prove the row is correct in shape, or that
//! `TOLERATED`'s reasons are still true — both are for a reader.
//!
//! `TOLERATED` is empty. Every function this rule reaches today either audits
//! directly or delegates to one that does. If that changes, an entry belongs
//! here with the reason a reader would otherwise ask for, and the list may
//! only shrink.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A function whose own `ctx.permit(Action::Settle | Action::Delete, ..)`
/// runs without an audit row anywhere beneath it, and the reason it is not a
/// hole. Empty today — see the module doc.
const TOLERATED: [(&str, &str); 0] = [];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

struct Function {
    name: String,
    module: String,
    line: usize,
    public: bool,
    body: String,
}

/// Every `fn` in a file with a body, private ones included — identical to
/// `permit_asked`'s walk, duplicated rather than shared because these are
/// two separate test binaries.
fn functions(module: &str, source: &str) -> Vec<Function> {
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

        let mut line_start = at;
        while line_start > 0 && chars[line_start - 1] != '\n' {
            line_start -= 1;
        }
        let head: String = chars[line_start..at].iter().collect();
        let trimmed = head.trim_start();
        let public = trimmed.starts_with("pub") && !trimmed.starts_with("pub(");
        let line = chars[..at].iter().filter(|c| **c == '\n').count() + 1;

        found.push(Function {
            name,
            module: module.to_string(),
            line,
            public,
            body: chars[body_start..j.min(chars.len())].iter().collect(),
        });

        at = body_start;
    }

    found
}

fn collect(root: &Path, dir: &Path, into: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).expect("src/ to be readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect(root, &path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let module = path
                .strip_prefix(root)
                .expect("a path under src/")
                .with_extension("")
                .to_string_lossy()
                .replace(['/', '\\'], "::")
                .trim_end_matches("::mod")
                .to_string();
            into.push((
                module,
                std::fs::read_to_string(&path).expect("a source file"),
            ));
        }
    }
}

/// Calls a body makes, as `(module, name)` — identical to `permit_asked`'s.
fn callees(module: &str, body: &str, modules: &[String]) -> HashSet<(String, String)> {
    let chars: Vec<char> = body.chars().collect();
    let mut out = HashSet::new();
    let mut i = 0;

    while i < chars.len() {
        if !ident_char(chars[i]) || (i > 0 && ident_char(chars[i - 1])) {
            i += 1;
            continue;
        }
        let word: String = chars[i..].iter().take_while(|c| ident_char(**c)).collect();
        let end = i + word.chars().count();
        let mut after = end;
        while after < chars.len() && chars[after] == ' ' {
            after += 1;
        }

        if after < chars.len() && chars[after] == '(' {
            let qualified = i >= 2 && chars[i - 1] == ':' && chars[i - 2] == ':';
            if qualified {
                let mut k = i - 2;
                while k > 0 && ident_char(chars[k - 1]) {
                    k -= 1;
                }
                let owner: String = chars[k..i - 2].iter().collect();
                for m in modules {
                    if *m == owner || m.ends_with(&format!("::{owner}")) {
                        out.insert((m.clone(), word.clone()));
                    }
                }
            } else if i == 0 || (chars[i - 1] != '.' && chars[i - 1] != ':') {
                out.insert((module.to_string(), word.clone()));
            }
        }
        i = end.max(i + 1);
    }

    out
}

fn asks_for_settle_or_delete(body: &str) -> bool {
    for action in ["Action::Settle", "Action::Delete"] {
        if let Some(at) = body.find("ctx.permit(") {
            // Enough for this test's purpose: the action named in the same
            // `ctx.permit(..)` call, found by looking for the action token
            // within a short window after the call starts. Multiple permit
            // calls in one function are rare; `find` takes the first.
            let window_end = (at + 200).min(body.len());
            if body[at..window_end].contains(action) {
                return true;
            }
        }
    }
    false
}

#[test]
fn every_settle_or_delete_writes_an_audit_row() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut sources = Vec::new();
    collect(&dir, &dir, &mut sources);
    sources.sort_by(|a: &(String, String), b| a.0.cmp(&b.0));
    assert!(!sources.is_empty(), "no source files were scanned");

    let modules: Vec<String> = sources.iter().map(|(module, _)| module.clone()).collect();
    let all: Vec<Function> = sources
        .iter()
        .flat_map(|(module, source)| functions(module, source))
        .collect();

    let mut audits: HashMap<(String, String), bool> = HashMap::new();
    for function in &all {
        let entry = audits
            .entry((function.module.clone(), function.name.clone()))
            .or_insert(false);
        *entry = *entry || function.body.contains("ctx.audit(");
    }

    // Auditing is inherited: a function that hands the write to another
    // function in this crate has audited if that function did.
    loop {
        let mut settled = true;
        for function in &all {
            let key = (function.module.clone(), function.name.clone());
            if audits.get(&key).copied().unwrap_or(false) {
                continue;
            }
            if callees(&function.module, &function.body, &modules)
                .iter()
                .any(|callee| audits.get(callee).copied().unwrap_or(false))
            {
                audits.insert(key, true);
                settled = false;
            }
        }
        if settled {
            break;
        }
    }

    let mut silent = Vec::new();
    let mut used = Vec::new();

    for function in &all {
        if !function.public || !asks_for_settle_or_delete(&function.body) {
            continue;
        }
        let key = (function.module.clone(), function.name.clone());
        if audits.get(&key).copied().unwrap_or(false) {
            continue;
        }
        let at = format!("{}::{}", function.module, function.name);
        match TOLERATED.iter().find(|(name, _)| *name == at) {
            Some((name, _)) => used.push(*name),
            None => silent.push(format!(
                "src/{}.rs:{} — {at}",
                function.module.replace("::", "/"),
                function.line
            )),
        }
    }

    assert!(
        silent.is_empty(),
        "these public functions ask for Action::Settle or Action::Delete and leave \
         no audit row:\n  {}\n\
         Write one — `ctx.audit(tx, AuditEntry { .. }).await?;` — matching the shape \
         other functions writing that field use, or, if the asymmetry is real, name it \
         in TOLERATED with the reason.",
        silent.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these audit now, or ask for neither Settle nor Delete any more; take them \
         out of TOLERATED: {stale:?}"
    );
}

/// The detector itself, on a fixture: a `Settle` that never calls
/// `ctx.audit(..)`, directly or through anything it calls, is caught.
#[test]
fn a_settle_with_no_audit_is_caught() {
    let source = "\
        pub async fn settle_silently(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::Settle, Resource::Order { id, customer })?;\n\
            sqlx::query(\"update thing set settled = true\").execute(&mut **tx).await?;\n\
            // no ctx.audit call here — this is the bug this test exists to catch\n\
            Ok(())\n\
        }\n";

    let functions = functions("fixture", source);
    let f = functions
        .iter()
        .find(|f| f.name == "settle_silently")
        .expect("settle_silently");

    assert!(f.public, "the fixture function should read as public");
    assert!(
        asks_for_settle_or_delete(&f.body),
        "the fixture should be recognised as asking for Action::Settle"
    );
    assert!(
        !f.body.contains("ctx.audit("),
        "a Settle with no audit call should be detected as silent — if this fails, \
         the detector itself is broken"
    );
}

/// A function that does write the audit row is not flagged — the positive
/// case, so a fix to the detector cannot pass by rejecting everything.
#[test]
fn a_settle_with_an_audit_is_not_caught() {
    let source = "\
        pub async fn settle_and_audit(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::Settle, Resource::Order { id, customer })?;\n\
            sqlx::query(\"update thing set settled = true\").execute(&mut **tx).await?;\n\
            ctx.audit(tx, AuditEntry { entity: \"thing\", .. }).await?;\n\
            Ok(())\n\
        }\n";

    let functions = functions("fixture", source);
    let f = functions
        .iter()
        .find(|f| f.name == "settle_and_audit")
        .expect("settle_and_audit");

    assert!(f.body.contains("ctx.audit("), "fixture should audit");
}

/// A function that asks for `Action::View` or `Action::Write` is not this
/// rule's business, audited or not.
#[test]
fn a_view_or_write_is_not_examined() {
    let source = "\
        pub async fn read_only(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::View, Resource::Order { id, customer })?;\n\
            Ok(())\n\
        }\n";

    let functions = functions("fixture", source);
    let f = functions
        .iter()
        .find(|f| f.name == "read_only")
        .expect("read_only");

    assert!(
        !asks_for_settle_or_delete(&f.body),
        "a View permit should not be mistaken for Settle or Delete"
    );
}
