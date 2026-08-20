//! "Every interpolation inside a SQL `format!` is a constant, or it is named
//! and reasoned about."
//!
//! Issue #164 measured it: 169 queries in `src/` are built with
//! `sqlx::query*(&format!(..))`. Most of their interpolations are constants —
//! `{COLUMNS}`, `{ORDER_COLUMNS}` and the rest, a column list named once and
//! reused, which is why the `format!` is there at all. A private function
//! taking `column: &str` and dropping it into one of these strings is one
//! careless caller away from a hole, and the caller does not have to be
//! careless on purpose — it only has to pass something it received. Today
//! nothing does. This is prevention, not repair.
//!
//! Two checks, both reading `src/` rather than a list somebody maintains, so
//! a new `format!` is covered the day it is written:
//!
//! 1. [`every_dynamic_sql_interpolation_is_tolerated`] — every `{name}`
//!    inside a `format!` whose string looks like SQL (starts with `select`,
//!    `insert`, `update`, `delete` or `with`, case-insensitively) is either
//!    `SCREAMING_CASE` — a constant, resolved by reading the source, not by
//!    running it — or named in [`TOLERATED`] with the reason its value
//!    cannot come from a caller. The list may only shrink.
//!
//! 2. [`private_functions_reaching_sql_take_only_literals`] — the narrower
//!    property issue #164 asked for if it was cheap: a function whose body
//!    puts one of its own `&str`/`&'static str` parameters into a SQL
//!    `format!` is private, and every call to it in the same file passes a
//!    string literal for that parameter. It held for all eight such
//!    functions found (`cart::address_of`, `order::bump`, `order::touch`,
//!    `fulfilment::move_quantities`, `catalogue::refusal`,
//!    `catalogue::soft_delete`, `catalogue::link`, `catalogue::unlink`), so
//!    it is a hard assertion rather than a list — a function that fails it
//!    needs its call site fixed, not an entry excusing it.
//!
//! What this proves: a new dynamic fragment in a SQL `format!` fails CI
//! rather than review, and a new private helper string-splicing a caller's
//! `&str` into one does too, unless every call to it is a literal.
//!
//! What it does not prove, and why the boundary is where it is:
//!
//! - Check 1 matches by the placeholder's **name**, not its location — the
//!   way `tests/permit_asked.rs` matches calls by function name and says so.
//!   A new `{table}` anywhere in the crate, however it is built, passes
//!   silently, because `table` is already tolerated. The reason recorded for
//!   each name has to stay true of everywhere that name is used; a value
//!   found here that does not fit its reason is worth a human look, the same
//!   way `tests/reachable.rs`'s constraint check does not distinguish where a
//!   literal sits.
//! - It does not verify a `TOLERATED` reason — it only confirms the name is
//!   accounted for. A wrong reason and a right one look the same to this
//!   test.
//! - It finds SQL by a keyword prefix on the format string, not by tracing
//!   where the formatted value ends up. Nothing in `src/` today builds a
//!   query starting with anything else (no bare CTE, no stored procedure
//!   call, no DDL) — checked by hand, not by this test.
//! - It reads only `src/`. `tests/` is not scanned: a test building its own
//!   SQL is building it against its own fixture, and there is no host
//!   boundary there for a caller to smuggle a string through.
//! - Check 2 can only state the property for a dynamic name that is
//!   literally a function parameter. Eight of the seventeen tolerated names
//!   are not — `on_conflict`/`conflict_target`/`conflict` are `let` bindings
//!   chosen between literals by an `if`, `carried` is read from
//!   `information_schema`, `key`/`parent_table`/`adjustment_table`/
//!   `tax_table` are elements of a literal tuple returned by a private
//!   method's `match`, and `beyond`/`direction` are `let` bindings off
//!   `page::Order`'s two methods, each of which returns one of two literals
//!   and nothing else. Check 2 does not and cannot cover those; their safety
//!   rests on the reason in `TOLERATED` and a human reading it, same as
//!   always. `table`, `column`, and `parent` are each true for a mix of
//!   function-parameter and loop-variable occurrences, so check 2 covers
//!   only the parameter half of those two names.
//! - Check 2 finds a "call site" by the bare identifier `name(` at a word
//!   boundary, searched only in the same file as the definition. That is
//!   sound *because* every function it looks at is genuinely private (not
//!   even `pub(crate)`), so nothing outside that file can name it — a
//!   collision like `order::touch` and `workflow::touch`, two different
//!   private functions sharing a name in different files, is why the search
//!   is scoped to one file rather than the whole crate; checked by hand that
//!   this holds for all eight today.
//! - Fourteen entries were measured by hand in issue #164; this scan found a
//!   fifteenth — `parent_table`, the third element of the same
//!   `LineMoney::tables()` tuple as `key`, one line below it in
//!   `order::erase`, evidently missed by the manual count. `TOLERATED` seeds
//!   from what the scan finds, not from the issue text.

use std::collections::BTreeSet;
use std::path::Path;

/// Every dynamic (non-constant) interpolation this scan finds today inside a
/// SQL `format!` in `src/`, and why its value cannot come from a caller.
/// Adding to this is not a fix — a new entry has to argue for itself, the
/// same as `tests/permit_asked.rs`'s. The list may only shrink.
const TOLERATED: [(&str, &str); 17] = [
    (
        "beyond",
        "`page::Order::beyond`, which answers `\">\"` or `\"<\"` and nothing \
         else — the comparison a paged query makes against its cursor, picked \
         by which end of the list was asked for. The three lists that take an \
         `Order` read it off the filter they were handed; nothing reaches a \
         string in",
    ),
    (
        "direction",
        "`page::Order::direction`, which answers `\"asc\"` or `\"desc\"` and \
         nothing else, and is always used beside `beyond` — the two have to \
         agree or a list pages away from its own cursor, which is what \
         `page.rs`'s own test asserts",
    ),
    (
        "on_conflict",
        "picked by an `if`/`else if` between two or three `on conflict` \
         clauses written out in full in the same function — `cart::insert_line` \
         and the merge loop inside `cart::transfer_to_customer` — never a \
         value read from anywhere outside it",
    ),
    (
        "conflict_target",
        "`payout::set_commission_rule` picks between two `on conflict` \
         targets written out in full, by an `if` on whether the rule is \
         per-category — the same shape as `on_conflict`, named differently \
         because it lives in a different module",
    ),
    (
        "conflict",
        "`credit::put_cart_credit` picks between two partial-index conflict \
         targets the same way, one per credit instrument kind",
    ),
    (
        "carried",
        "`cart_line_item`'s own column names, read from \
         `information_schema.columns` at query time and joined into one list \
         — issue #140's fix. The function that builds it takes no column list \
         from a caller at all",
    ),
    (
        "which",
        "the sole `&str` parameter of the private `cart::address_of`; both \
         call sites pass a literal `coalesce(..)` expression naming which of \
         a cart's two addresses to read",
    ),
    (
        "table",
        "either a private function's own `&'static str` parameter — \
         `catalogue::refusal`, `catalogue::soft_delete`, `catalogue::link`, \
         `catalogue::unlink` — with every call site literal, or a `for` loop \
         over an array literal written in the same function — `order::erase`, \
         the order change handler for `ShippingRemove` — never a value a \
         caller chose",
    ),
    (
        "column",
        "the same two shapes as `table`: a private function's own \
         `&'static str`/`&str` parameter with every call site literal — \
         `catalogue::link`, `catalogue::unlink`, `order::bump`, \
         `fulfilment::move_quantities` — or an element of a literal \
         tuple/array in the same function — `order::insert_line_money`'s \
         `LineMoney::tables()`, the per-owner loop in `tax.rs`",
    ),
    (
        "ceiling",
        "the private `order::bump`'s and `fulfilment::move_quantities`'s own \
         `&str` parameter; every call site passes one of a fixed set of \
         column-name literals, e.g. `bump(.., \"return_requested_quantity\", \
         \"quantity\", ..)`",
    ),
    (
        "assignment",
        "the private `order::touch`'s sole `&str` parameter, a full \
         `column = $4`-shaped assignment; its one call site passes a literal",
    ),
    (
        "parent",
        "an element of the two-item array literal `tax.rs` loops over when \
         writing tax lines for a cart's items and its shipping methods — \
         never a value read from a caller",
    ),
    (
        "key",
        "the fourth element of the private `LineMoney::tables()`'s literal \
         tuple, matched in `order::erase`; the method has exactly two arms \
         and both return literal `&'static str` tuples",
    ),
    (
        "parent_table",
        "the third element of that same `LineMoney::tables()` tuple, used \
         one line below `key` in `order::erase` — not in issue #164's own \
         list of fourteen; this scan found it, the manual count missed it",
    ),
    (
        "cols",
        "`subscription::due_deliveries` binds the placeholder explicitly — \
         `cols = SUBSCRIPTION_COLUMNS` — to a `SCREAMING_CASE` constant; the \
         placeholder itself is lower case only because it reads better next \
         to `subscription s` in the query",
    ),
    (
        "adjustment_table",
        "the first element of `LineMoney::tables()`'s literal tuple, matched \
         in `order::insert_line_money`",
    ),
    (
        "tax_table",
        "the second element of that same tuple, matched one line below \
         `adjustment_table`",
    ),
];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_constant_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// The index one past the `)` matching the `(` at `open`, skipping the
/// contents of `"..."` string literals. `None` if it never closes.
fn matching_close_paren(chars: &[char], open: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = open;
    let mut in_str = false;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
        } else {
            match c {
                '"' => in_str = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

/// The first `"..."` string literal at the start of `chars` (leading
/// whitespace allowed), unescaped quoting aside — its raw text between the
/// quotes.
fn leading_string_literal(chars: &[char]) -> Option<String> {
    let mut i = 0;
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if chars.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    let start = i;
    let mut escape = false;
    while i < chars.len() {
        let c = chars[i];
        if escape {
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if c == '"' {
            return Some(chars[start..i].iter().collect());
        }
        i += 1;
    }
    None
}

fn is_sql_shaped(literal: &str) -> bool {
    let lower = literal.trim_start().to_ascii_lowercase();
    ["select", "insert", "update", "delete", "with "]
        .iter()
        .any(|kw| lower.starts_with(kw))
}

/// Every named `{ident}` placeholder in a format string, `{{`/`}}` escapes
/// skipped, in the order they appear (a name repeated in one string is
/// repeated here too).
fn placeholders(literal: &str) -> Vec<String> {
    let chars: Vec<char> = literal.chars().collect();
    let mut names = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '{' if chars.get(i + 1) == Some(&'{') => i += 2,
            '{' => {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != '}' {
                    j += 1;
                }
                if j < chars.len() {
                    let inner: String = chars[i + 1..j].iter().collect();
                    let name = inner.split(':').next().unwrap_or("").trim();
                    if name
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                        && name.chars().all(ident_char)
                    {
                        names.push(name.to_string());
                    }
                    i = j + 1;
                } else {
                    i += 1;
                }
            }
            '}' if chars.get(i + 1) == Some(&'}') => i += 2,
            _ => i += 1,
        }
    }
    names
}

/// Split `s` on its top-level commas — depth tracked over `(`, `[`, `<`,
/// `{`, string contents skipped. Used for both a parameter list (no strings
/// in a type) and a call's argument list (strings very much present).
fn split_top_level(s: &str) -> Vec<String> {
    let chars: Vec<char> = s.chars().collect();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let mut start = 0usize;
    let mut out = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            ',' if depth == 0 => {
                out.push(
                    chars[start..i]
                        .iter()
                        .collect::<String>()
                        .trim()
                        .to_string(),
                );
                start = i + 1;
            }
            _ => {}
        }
    }
    let last: String = chars[start..].iter().collect::<String>().trim().to_string();
    if !last.is_empty() {
        out.push(last);
    }
    out
}

struct Interpolation {
    line: usize,
    name: String,
}

/// Every non-constant `{name}` inside a SQL-shaped `format!` anywhere in
/// `source`, deduplicated within each call.
fn dynamic_sql_interpolations(source: &str) -> Vec<Interpolation> {
    let chars: Vec<char> = source.chars().collect();
    let marker: Vec<char> = "format!(".chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + marker.len() <= chars.len() {
        if &chars[i..i + marker.len()] != marker.as_slice() {
            i += 1;
            continue;
        }
        let open = i + marker.len() - 1;
        if let Some(close) = matching_close_paren(&chars, open) {
            let body = &chars[open + 1..close];
            if let Some(literal) = leading_string_literal(body)
                && is_sql_shaped(&literal)
            {
                let line = chars[..i].iter().filter(|c| **c == '\n').count() + 1;
                let mut seen = BTreeSet::new();
                for name in placeholders(&literal) {
                    if !is_constant_name(&name) && seen.insert(name.clone()) {
                        out.push(Interpolation { line, name });
                    }
                }
            }
        }
        i += marker.len();
    }
    out
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

#[test]
fn every_dynamic_sql_interpolation_is_tolerated() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    collect(&dir, &dir, &mut sources);
    assert!(!sources.is_empty(), "no source files were scanned");

    let mut silent = Vec::new();
    let mut found_names: BTreeSet<String> = BTreeSet::new();

    for (module, source) in &sources {
        for interp in dynamic_sql_interpolations(source) {
            found_names.insert(interp.name.clone());
            if !TOLERATED.iter().any(|(name, _)| *name == interp.name) {
                silent.push(format!(
                    "src/{}.rs:{} — {{{}}}",
                    module.replace("::", "/"),
                    interp.line,
                    interp.name
                ));
            }
        }
    }

    assert!(
        silent.is_empty(),
        "these SQL `format!` interpolations are not `SCREAMING_CASE` \
         constants and are not in `TOLERATED`:\n  {}\n\
         Either name the value as a constant, or add it to `TOLERATED` with \
         the reason its value cannot come from a caller.",
        silent.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !found_names.contains(*name))
        .collect();
    assert!(
        stale.is_empty(),
        "these interpolations are gone, or are `SCREAMING_CASE` now; take \
         them out of `TOLERATED`: {stale:?}"
    );
}

struct FunctionSig {
    name: String,
    line: usize,
    public: bool,
    params: String,
    body: String,
}

/// Every `fn`/`async fn` in `source`, its parameter list and body kept apart
/// so a parameter's name can be checked against what the body does with it.
fn function_signatures(source: &str) -> Vec<FunctionSig> {
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

        let mut name_at = at + 2;
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

        let mut p = name_at + name.chars().count();
        while p < chars.len() && chars[p] != '(' && chars[p] != '{' && chars[p] != ';' {
            p += 1;
        }
        let params_end = if chars.get(p) == Some(&'(') {
            matching_close_paren(&chars, p)
        } else {
            None
        };
        let Some(params_end) = params_end else {
            at = name_at;
            continue;
        };
        let params: String = chars[p + 1..params_end].iter().collect();

        let mut depth = 0i32;
        let mut i = params_end + 1;
        let mut body_start = None;
        while i < chars.len() {
            match chars[i] {
                '<' | '[' => depth += 1,
                '>' | ']' => depth -= 1,
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
            at = params_end + 1;
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
        // `pub(crate)`/`pub(super)` cannot be named from another file, so a
        // truly private function and one of these are the same thing here.
        let public = trimmed.starts_with("pub") && !trimmed.starts_with("pub(");
        let line = chars[..at].iter().filter(|c| **c == '\n').count() + 1;

        found.push(FunctionSig {
            name,
            line,
            public,
            params,
            body: chars[body_start..j.min(chars.len())].iter().collect(),
        });

        at = body_start;
    }

    found
}

/// The parameter's name if `param` (one entry from a split parameter list)
/// declares it `&str` or `&'static str` — any lifetime, any name.
fn str_param_name(param: &str) -> Option<String> {
    let (name, ty) = param.split_once(':')?;
    let name = name.trim().strip_prefix("mut ").unwrap_or(name.trim());
    if name.is_empty() || name == "self" {
        return None;
    }
    let ty = ty.trim().strip_prefix('&')?.trim_start();
    let ty = if let Some(rest) = ty.strip_prefix('\'') {
        rest.trim_start_matches(ident_char).trim_start()
    } else {
        ty
    };
    if ty == "str" {
        Some(name.to_string())
    } else {
        None
    }
}

/// `true` if `chars[at..]` starts with the word `word` at a word boundary —
/// not preceded and not followed by an identifier character.
fn word_at(chars: &[char], at: usize, word: &[char]) -> bool {
    if at + word.len() > chars.len() {
        return false;
    }
    if at > 0 && ident_char(chars[at - 1]) {
        return false;
    }
    if &chars[at..at + word.len()] != word {
        return false;
    }
    !chars.get(at + word.len()).is_some_and(|c| ident_char(*c))
}

/// Call sites of `fn_name(` in `source`, its own `fn` declaration excluded,
/// as the raw text of the call's argument list.
fn call_sites(source: &str, fn_name: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let name_chars: Vec<char> = fn_name.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !word_at(&chars, i, &name_chars) {
            i += 1;
            continue;
        }
        let after = i + name_chars.len();
        let mut open = after;
        while open < chars.len() && chars[open].is_whitespace() {
            open += 1;
        }
        if chars.get(open) != Some(&'(') {
            i = after;
            continue;
        }
        // Skip the definition itself: `fn NAME(`, whitespace already spanned.
        let mut before = i;
        while before > 0 && chars[before - 1].is_whitespace() {
            before -= 1;
        }
        let is_fn_keyword = before >= 2
            && chars[before - 2] == 'f'
            && chars[before - 1] == 'n'
            && !(before >= 3 && ident_char(chars[before - 3]));
        if is_fn_keyword {
            i = open + 1;
            continue;
        }
        if let Some(close) = matching_close_paren(&chars, open) {
            out.push(chars[open + 1..close].iter().collect::<String>());
            i = close + 1;
        } else {
            i = open + 1;
        }
    }
    out
}

fn is_string_literal(arg: &str) -> bool {
    let arg = arg.trim();
    (arg.starts_with('"') && arg.ends_with('"') && arg.len() >= 2)
        || (arg.starts_with("r\"") && arg.ends_with('"') && arg.len() >= 3)
        || (arg.starts_with("r#") && arg.ends_with('#'))
}

#[test]
fn private_functions_reaching_sql_take_only_literals() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources = Vec::new();
    collect(&dir, &dir, &mut sources);
    assert!(!sources.is_empty(), "no source files were scanned");

    let mut made_public = Vec::new();
    let mut not_literal = Vec::new();
    let mut checked = 0usize;

    for (module, source) in &sources {
        for function in function_signatures(source) {
            let params = split_top_level(&function.params);
            let str_params: Vec<(usize, String)> = params
                .iter()
                .enumerate()
                .filter_map(|(idx, p)| str_param_name(p).map(|name| (idx, name)))
                .collect();
            if str_params.is_empty() {
                continue;
            }

            let reaching: BTreeSet<String> = dynamic_sql_interpolations(&function.body)
                .into_iter()
                .map(|interp| interp.name)
                .collect();
            let reaching_params: Vec<(usize, String)> = str_params
                .into_iter()
                .filter(|(_, name)| reaching.contains(name))
                .collect();
            if reaching_params.is_empty() {
                continue;
            }

            checked += 1;
            let at = format!(
                "src/{}.rs:{} — {}",
                module.replace("::", "/"),
                function.line,
                function.name
            );

            if function.public {
                made_public.push(at.clone());
                continue;
            }

            for (idx, param_name) in &reaching_params {
                for call in call_sites(source, &function.name) {
                    let Some(arg) = split_top_level(&call).into_iter().nth(*idx) else {
                        continue;
                    };
                    if !is_string_literal(&arg) {
                        not_literal.push(format!("{at} — `{param_name}` got `{arg}`"));
                    }
                }
            }
        }
    }

    assert!(
        checked > 0,
        "no private function's `&str` parameter was found reaching a SQL \
         `format!` — the scan itself is broken, since issue #164 named eight"
    );

    assert!(
        made_public.is_empty(),
        "these functions let a `&str` parameter reach a SQL `format!` and \
         are reachable from outside the crate — make them private, or stop \
         interpolating the parameter:\n  {}",
        made_public.join("\n  ")
    );

    assert!(
        not_literal.is_empty(),
        "these calls pass something other than a string literal for a \
         parameter that reaches a SQL `format!` — pass a literal, or this is \
         the hole issue #164 was checking for:\n  {}",
        not_literal.join("\n  ")
    );
}

#[cfg(test)]
mod fixtures {
    use super::*;

    #[test]
    fn a_callers_string_spliced_into_sql_is_found() {
        let source = r#"
            async fn broken(tx: &mut Tx<'_>, sort: &str) -> Result<()> {
                sqlx::query(&format!("select * from cart order by {sort}"))
                    .fetch_all(tx)
                    .await?;
                Ok(())
            }
        "#;
        let found = dynamic_sql_interpolations(source);
        assert!(
            found.iter().any(|i| i.name == "sort"),
            "a lower-case interpolation must be found, the way `every_dynamic_sql_interpolation_is_tolerated` \
             would then fail the build because `sort` is not in `TOLERATED`"
        );
    }

    #[test]
    fn a_constant_interpolation_passes() {
        let source = r#"
            async fn fine(tx: &mut Tx<'_>) -> Result<()> {
                sqlx::query_as::<_, Row>(&format!("select {COLUMNS} from cart where scope = $1"))
                    .fetch_all(tx)
                    .await?;
                Ok(())
            }
        "#;
        let found = dynamic_sql_interpolations(source);
        assert!(
            found.is_empty(),
            "a `SCREAMING_CASE` interpolation is a constant, not a caller's value: {:?}",
            found.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_non_literal_call_site_is_caught() {
        let source = r#"
            async fn narrow(tx: &mut Tx<'_>, ctx: &Ctx<'_>, column: &str) -> Result<()> {
                sqlx::query(&format!("update thing set {column} = $1"))
                    .bind(1)
                    .execute(tx)
                    .await?;
                Ok(())
            }

            async fn caller(tx: &mut Tx<'_>, ctx: &Ctx<'_>, requested: &str) -> Result<()> {
                narrow(tx, ctx, requested).await
            }
        "#;

        let functions = function_signatures(source);
        let narrow = functions
            .iter()
            .find(|f| f.name == "narrow")
            .expect("narrow");
        assert!(
            !narrow.public,
            "narrow must be private for this fixture to prove anything"
        );

        let params = split_top_level(&narrow.params);
        let str_params: Vec<(usize, String)> = params
            .iter()
            .enumerate()
            .filter_map(|(idx, p)| str_param_name(p).map(|name| (idx, name)))
            .collect();
        let reaching: BTreeSet<String> = dynamic_sql_interpolations(&narrow.body)
            .into_iter()
            .map(|i| i.name)
            .collect();
        let (idx, _) = str_params
            .iter()
            .find(|(_, name)| reaching.contains(name))
            .expect("`column` reaches the format!");

        let calls = call_sites(source, "narrow");
        assert_eq!(calls.len(), 1, "one call site outside the definition");
        let arg = split_top_level(&calls[0])
            .into_iter()
            .nth(*idx)
            .expect("the column argument");
        assert!(
            !is_string_literal(&arg),
            "`requested` is a variable, not a literal — this call site must fail the check"
        );
    }
}
