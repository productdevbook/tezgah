//! "Every function that asks for `Action::Settle` or `Action::Delete` writes
//! an audit row for the thing it changed."
//!
//! `tests/audit_symmetry.rs` watches five hand-named pairs, each found by
//! reading two functions side by side. That is real work and it does not
//! scale: the next asymmetric pair is found the same way, by hand, and only
//! after it has shipped. `ports.rs` already says what `Settle` and `Delete`
//! are — moving money, and destroying a row — and both are, by definition,
//! acts an operator has to be able to reconstruct afterwards. That gives a
//! rule that needs no pairing at all: ask for one of those two and the audit
//! sink hears about it, for the row that was actually touched.
//!
//! This reuses `tests/permit_asked.rs`'s extraction — the same brace-matching
//! walk over every `fn`, the same call-graph reachability — pointed at a
//! different question. `permit_asked` asks "did somebody ask before this ran
//! a query"; this asks "did the function that asked for `Settle` or `Delete`
//! leave a trail for the row it changed." Asking is inherited, but only when
//! the subject matches: a function that hands the actual write to another
//! function in this crate has audited if that function did, and if the table
//! the helper's audit names is a table this function itself writes — or, when
//! this function does no writing of its own and is a pure pass-through, any
//! table the helper names, because there is nothing local to disagree with.
//!
//! What it proves: every public function whose own `ctx.permit(..)` names
//! `Action::Settle` or `Action::Delete` either calls `ctx.audit(..)` itself,
//! naming any entity, or reaches an `ctx.audit(..)` call — through a function
//! this crate calls — whose `entity` names a table this function's own SQL
//! also writes (or, if this function writes nothing itself, any entity the
//! callee reached).
//!
//! What it does not prove: that the audit row's `entity` is the *correct*
//! table when a function's own SQL legitimately touches more than one — this
//! reads whichever table names appear in `delete from` / `update` / `insert
//! into` and accepts a match against any of them. It also does not prove the
//! row is correct in shape, that its `entity_id` is the row this call actually
//! changed, or that `TOLERATED`'s reasons are still true — those are for a
//! reader. And the call graph is still untyped text matching: a private
//! helper with the right name in the right module reads as the same helper
//! wherever it is called from.
//!
//! It was caught, before this fix existed, only by reading all eleven
//! inherited-only passes by hand: `cart::expire` inherited a pass through
//! `inventory::release_cart`, which audits `reservation_item` — releasing a
//! hold — on its way to `cart::expire`'s own `delete from cart`. Ten of the
//! eleven were genuine delegation to a same-subject helper; that one was a
//! coincidence of call order. `cart::expire` has carried its own `ctx.audit`
//! call, naming `"cart"`, since #156 — this rule no longer needs to trust the
//! inherited path to cover it.
//!
//! `TOLERATED` is empty. Every function this rule reaches today either audits
//! directly or delegates to a helper auditing the same subject. If that
//! changes, an entry belongs here with the reason a reader would otherwise
//! ask for, and the list may only shrink.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A function whose own `ctx.permit(Action::Settle | Action::Delete, ..)`
/// runs without a same-subject audit row anywhere beneath it, and the reason
/// it is not a hole. Empty today — see the module doc.
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

/// A function's own `ctx.permit(..)` names `Action::Settle` or
/// `Action::Delete` — checked at every `ctx.permit(` call in the body, not
/// only the first, so a function that asks more than once (a `View` to look,
/// then a `Settle` to act) is not missed because the first call named
/// something else.
fn asks_for_settle_or_delete(body: &str) -> bool {
    for at in permit_call_starts(body) {
        let window_end = (at + 200).min(body.len());
        if body[at..window_end].contains("Action::Settle")
            || body[at..window_end].contains("Action::Delete")
        {
            return true;
        }
    }
    false
}

/// The byte offset of every `ctx.permit(` call in a body.
fn permit_call_starts(body: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = body[from..].find("ctx.permit(") {
        let at = from + pos;
        out.push(at);
        from = at + "ctx.permit(".len();
    }
    out
}

/// The tables a body's own SQL writes: the word after `delete from`,
/// `insert into`, or a bare `update` not immediately followed by `set` (an
/// `on conflict do update set` names no table of its own — the table is
/// whichever the surrounding `insert into` named). This only sees a
/// function's own text, never a callee's, because `functions` slices bodies
/// at their own braces.
fn written_entities(body: &str) -> HashSet<String> {
    let words: Vec<&str> = body
        .split(|c: char| !ident_char(c))
        .filter(|w| !w.is_empty())
        .collect();
    let mut out = HashSet::new();
    let mut i = 0;
    while i < words.len() {
        match words[i] {
            "delete" if words.get(i + 1) == Some(&"from") => {
                if let Some(table) = words.get(i + 2) {
                    out.insert((*table).to_string());
                }
            }
            "insert" if words.get(i + 1) == Some(&"into") => {
                if let Some(table) = words.get(i + 2) {
                    out.insert((*table).to_string());
                }
            }
            "update" if words.get(i + 1) != Some(&"set") => {
                if let Some(table) = words.get(i + 1) {
                    out.insert((*table).to_string());
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// A marker for a `ctx.audit(..)` call whose `entity` could not be read as a
/// string literal — treated as matching anything, so a shape the parser
/// cannot follow fails open onto the old, coarser check rather than reporting
/// a hole that is not one.
const UNRESOLVED_ENTITY: &str = "*";

/// The `entity` literal of every `ctx.audit(..)` call in a body's own text.
fn audited_entities(body: &str) -> HashSet<String> {
    let bytes = body.as_bytes();
    let mut out = HashSet::new();
    let mut from = 0;
    while let Some(pos) = body[from..].find("ctx.audit(") {
        let open = from + pos + "ctx.audit(".len() - 1;

        let mut depth = 0i32;
        let mut j = open;
        let mut close = None;
        while j < bytes.len() {
            match bytes[j] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(j);
                        break;
                    }
                }
                _ => {}
            }
            j += 1;
        }

        let span = match close {
            Some(end) => &body[open..=end],
            None => &body[open..],
        };

        match entity_literal(span) {
            Some(entity) => {
                out.insert(entity);
            }
            None => {
                out.insert(UNRESOLVED_ENTITY.to_string());
            }
        }

        from = from + pos + "ctx.audit(".len();
    }
    out
}

/// The string literal following an `entity:` field in a `ctx.audit(..)` call
/// span.
fn entity_literal(span: &str) -> Option<String> {
    let after = span.split("entity:").nth(1)?;
    let start = after.find('"')? + 1;
    let rest = &after[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Whether an audited entity — possibly `UNRESOLVED_ENTITY` — counts as
/// covering a function whose own writes are `own`. A pure pass-through
/// (`own` empty) accepts anything a callee audited, because there is nothing
/// local to compare it against.
fn entity_matches(entity: &str, own: &HashSet<String>) -> bool {
    entity == UNRESOLVED_ENTITY || own.is_empty() || own.contains(entity)
}

/// For every function, the set of entities its own `ctx.audit(..)` calls
/// name, plus — for functions with none of their own — whatever a callee
/// audited that also matches a table this function itself writes (or,
/// writing nothing itself, whatever a callee audited at all).
///
/// A function's audited-entities set is non-empty exactly when
/// `every_settle_or_delete_writes_an_audit_row` should treat it as covered.
fn compute_audited_entities(
    all: &[Function],
    modules: &[String],
) -> HashMap<(String, String), HashSet<String>> {
    let mut own: HashMap<(String, String), HashSet<String>> = HashMap::new();
    let mut written: HashMap<(String, String), HashSet<String>> = HashMap::new();
    for function in all {
        let key = (function.module.clone(), function.name.clone());
        own.entry(key.clone())
            .or_default()
            .extend(audited_entities(&function.body));
        written
            .entry(key)
            .or_default()
            .extend(written_entities(&function.body));
    }

    let mut audited = own.clone();

    loop {
        let mut settled = true;
        for function in all {
            let key = (function.module.clone(), function.name.clone());
            if !audited.get(&key).is_none_or(HashSet::is_empty) {
                // Already directly audited; inheritance adds nothing a
                // reader needs.
                continue;
            }
            let own_written = written.get(&key).cloned().unwrap_or_default();
            let mut gained = Vec::new();
            for callee in callees(&function.module, &function.body, modules) {
                let Some(callee_entities) = audited.get(&callee) else {
                    continue;
                };
                for entity in callee_entities {
                    if entity_matches(entity, &own_written) {
                        gained.push(entity.clone());
                    }
                }
            }
            if !gained.is_empty() {
                let entry = audited.entry(key).or_default();
                for entity in gained {
                    if entry.insert(entity) {
                        settled = false;
                    }
                }
            }
        }
        if settled {
            break;
        }
    }

    audited
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

    let audited = compute_audited_entities(&all, &modules);

    let mut silent = Vec::new();
    let mut used = Vec::new();

    for function in &all {
        if !function.public || !asks_for_settle_or_delete(&function.body) {
            continue;
        }
        let key = (function.module.clone(), function.name.clone());
        if audited.get(&key).is_some_and(|e| !e.is_empty()) {
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
         no same-subject audit row:\n  {}\n\
         Write one — `ctx.audit(tx, AuditEntry {{ .. }}).await?;` — naming the table \
         this function itself writes, or, if the asymmetry is real, name it in \
         TOLERATED with the reason.",
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

/// A function whose second `ctx.permit(..)` names `Action::Settle`, after a
/// first that named `Action::View`, is caught — the window used to be
/// anchored on the first call only, and a function like this slipped past it
/// silently.
#[test]
fn a_settle_named_by_the_second_permit_is_seen() {
    let source = "\
        pub async fn look_then_settle(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::View, Resource::Order { id, customer })?;\n\
            let _: Permit = ctx.permit(Action::Settle, Resource::Order { id, customer })?;\n\
            sqlx::query(\"update thing set settled = true\").execute(&mut **tx).await?;\n\
            Ok(())\n\
        }\n";

    let functions = functions("fixture", source);
    let f = functions
        .iter()
        .find(|f| f.name == "look_then_settle")
        .expect("look_then_settle");

    assert!(
        asks_for_settle_or_delete(&f.body),
        "a Settle named by the second ctx.permit(..) call must not be missed"
    );
}

/// The shape of the bug this issue exists to fix: a function deletes its own
/// row, but the only audit call reachable from it — through a callee — names
/// a different table. Reachability alone used to pass this; matching
/// subjects must not.
#[test]
fn inheriting_a_different_entity_is_caught() {
    let caller = "\
        pub async fn expire(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::Delete, Resource::Customer { id: None })?;\n\
            release_cart(tx, ctx, id).await?;\n\
            sqlx::query(\"delete from cart where scope = $1 and id = $2\").execute(&mut **tx).await?;\n\
            Ok(())\n\
        }\n";
    let callee = "\
        pub async fn release_cart(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::Write, Resource::Inventory { id: None })?;\n\
            sqlx::query(\"delete from reservation_item where scope = $1\").execute(&mut **tx).await?;\n\
            ctx.audit(tx, AuditEntry { entity: \"reservation_item\", .. }).await?;\n\
            Ok(())\n\
        }\n";

    let mut all = functions("fixture", caller);
    all.extend(functions("fixture", callee));
    let modules = vec!["fixture".to_string()];

    let audited = compute_audited_entities(&all, &modules);
    let key = ("fixture".to_string(), "expire".to_string());
    assert!(
        audited.get(&key).is_none_or(HashSet::is_empty),
        "a Delete on `cart` reached only through a helper auditing \
         `reservation_item` must not read as covered"
    );
}

/// The paired positive: a caller and a same-subject helper still pass —
/// genuine delegation is not collateral damage from the fix above.
#[test]
fn inheriting_the_same_entity_still_passes() {
    let caller = "\
        pub async fn delete_gift_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>) -> Result<()> {\n\
            let _: Permit = ctx.permit(Action::Delete, Resource::GiftCard { id })?;\n\
            remove_gift_card(tx, ctx, id).await?;\n\
            Ok(())\n\
        }\n";
    let callee = "\
        async fn remove_gift_card(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: GiftCardId) -> Result<()> {\n\
            sqlx::query(\"delete from gift_card where scope = $1 and id = $2\").execute(&mut **tx).await?;\n\
            ctx.audit(tx, AuditEntry { entity: \"gift_card\", .. }).await?;\n\
            Ok(())\n\
        }\n";

    let mut all = functions("fixture", caller);
    all.extend(functions("fixture", callee));
    let modules = vec!["fixture".to_string()];

    let audited = compute_audited_entities(&all, &modules);
    let key = ("fixture".to_string(), "delete_gift_card".to_string());
    assert!(
        audited.get(&key).is_some_and(|e| !e.is_empty()),
        "a Delete on `gift_card` reached through a helper auditing the same \
         table should still read as covered, even though the caller's own \
         body names no table itself"
    );
}
