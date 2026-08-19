//! "It asks before it answers, on the branch where there is nothing to
//! answer about."
//!
//! `refund_to_credit` was unreachable until #161 gave it a route. Wiring it
//! surfaced a defect the deny-everything matrix in `api_permissions.rs` could
//! never have found, because that matrix only calls what already has a
//! route: the function read a row, and only asked the host once it had one —
//! so a nonexistent order answered `not_found` without ever being refused.
//! #162 is that defect turned into a rule.
//!
//! The matrix proves routes. This proves a narrower, cheaper thing about
//! every public function whether or not a route reaches it: on the branch
//! where a lookup came back empty, has the host already been asked? That is
//! one property, local to the function that actually decides `not_found`,
//! and it needs no database and no route — the source says it or it does
//! not.
//!
//! # What this proves
//!
//! For a public function outside `src/api/` whose own body answers
//! `Error::not_found(..)`: either `ctx.permit(..)` appears earlier in that
//! same body, or the function's first statement is a call to a private
//! function in the same module that itself asks before answering — the
//! `cart::open`, `fulfilment::load` shape this crate already uses to load a
//! row and judge it in one place. Chase that one hop, recursively, and stop:
//! a function that decides `not_found` two calls deep behind something else
//! is invisible to this check, and so is one reached only conditionally
//! rather than as the first thing the function does.
//!
//! # What this does not prove
//!
//! Ordering, not correctness of the `Resource` asked about — a function that
//! asks about the wrong id, or the wrong action, passes this exactly as
//! `tests/permit_asked.rs` already says of itself. And nothing about a
//! function with no `not_found` of its own and no callee this check
//! recognises: `workflow::work` drives runs by an id it read from its own
//! queue, not one a caller handed it, so it has nothing here to ask about,
//! and this file does not manufacture a question for it. A rule that chased
//! every callee, not just a function's first statement, would ask that
//! question anyway and flag it — which is exactly why the chase stops at one
//! hop, on purpose, rather than reading arbitrarily far into what a function
//! calls.
//!
//! Matched textually, the way every ratchet in this crate is: a private
//! function with the same name in two modules would be resolved to whichever
//! this file's module map holds for that module, which is correct here
//! because calls are already scoped to one module before being looked up.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Public functions whose own body answers `not_found` before any permit is
/// provably asked, each with the reason it is not a hole. Adding to this is
/// not a fix — see `tests/reachable.rs` for the shape this list follows.
const TOLERATED: [(&str, &str); 0] = [];

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[derive(Clone)]
struct Function {
    module: String,
    name: String,
    line: usize,
    public: bool,
    has_ctx: bool,
    body: String,
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// `src/` minus `src/api/`: the route table is what `api_permissions.rs`
/// proves, and is out of scope here on purpose.
fn files(dir: &Path, into: &mut Vec<(PathBuf, String)>) {
    for entry in std::fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "api") {
                continue;
            }
            files(&path, into);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("a readable file");
            into.push((path, text));
        }
    }
}

fn module(path: &Path) -> String {
    let stem = path
        .file_stem()
        .expect("a file name")
        .to_string_lossy()
        .to_string();
    if stem == "mod" {
        path.parent()
            .and_then(|p| p.file_name())
            .expect("a parent directory")
            .to_string_lossy()
            .to_string()
    } else {
        stem
    }
}

/// Every `fn` in a file with a body, private ones included — the same shape
/// `tests/permit_asked.rs` parses, kept separate because that file answers a
/// different question and no test here should depend on another compiling.
fn functions_in(module: &str, source: &str) -> Vec<Function> {
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
        let sig: String = chars[at..body_start].iter().collect();

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
            module: module.to_string(),
            name,
            line,
            public,
            has_ctx: sig.contains("ctx") && (sig.contains("&Ctx") || sig.contains("& Ctx")),
            body: chars[body_start..j.min(chars.len())].iter().collect(),
        });

        at = body_start;
    }

    found
}

fn all_functions() -> Vec<Function> {
    let mut sources = Vec::new();
    files(&root().join("src"), &mut sources);
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!sources.is_empty(), "no source files were scanned");

    sources
        .iter()
        .flat_map(|(path, text)| functions_in(&module(path), text))
        .collect()
}

/// The first statement's call, if it is one: `open(tx, ctx, id, ..).await?;`
/// or `let cart = open(tx, ctx, id, ..).await?;` both give `open`. Nothing
/// past the first top-level `;` is looked at — a gate called anywhere but
/// first is invisible to this, on purpose, so a conditional call cannot be
/// mistaken for one that always runs first.
fn first_statement_call(body: &str) -> Option<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] != '{' {
        i += 1;
    }
    i += 1;
    let stmt_start = i;
    let mut depth = 0i32;
    while i < chars.len() {
        match chars[i] {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ';' if depth <= 0 => break,
            _ => {}
        }
        i += 1;
    }
    let statement: String = chars[stmt_start..i].iter().collect();

    let s: Vec<char> = statement.chars().collect();
    let mut k = 0;
    while k < s.len() {
        if ident_char(s[k]) && (k == 0 || !ident_char(s[k - 1])) {
            let word: String = s[k..].iter().take_while(|c| ident_char(**c)).collect();
            let end = k + word.chars().count();
            if s.get(end) == Some(&'(') && word != "let" {
                let before_is_path = k >= 2 && s[k - 1] == ':' && s[k - 2] == ':';
                if !before_is_path {
                    return Some(word);
                }
            }
            k = end;
        } else {
            k += 1;
        }
    }
    None
}

enum Verdict {
    /// No `not_found(` in the reachable body: nothing for this check to ask
    /// about.
    NotApplicable,
    /// Asks before it can answer `not_found`, directly or through a gate
    /// called first.
    Safe,
    /// Answers `not_found` and nothing establishes that the host was asked
    /// first.
    Unsafe,
}

fn verdict(
    module: &str,
    name: &str,
    by_module: &HashMap<(String, String), Function>,
    visited: &mut HashSet<(String, String)>,
) -> Verdict {
    let key = (module.to_string(), name.to_string());
    if !visited.insert(key.clone()) {
        return Verdict::NotApplicable;
    }
    let Some(function) = by_module.get(&key) else {
        return Verdict::NotApplicable;
    };
    if !function.has_ctx {
        return Verdict::NotApplicable;
    }
    let Some(not_found_at) = function.body.find("not_found(") else {
        return Verdict::NotApplicable;
    };
    if let Some(permit_at) = function.body.find("ctx.permit(") {
        if permit_at < not_found_at {
            return Verdict::Safe;
        }
    }
    match first_statement_call(&function.body) {
        Some(callee) => match verdict(module, &callee, by_module, visited) {
            Verdict::Safe => Verdict::Safe,
            _ => Verdict::Unsafe,
        },
        None => Verdict::Unsafe,
    }
}

#[test]
fn every_public_function_asks_before_it_answers_not_found() {
    let functions = all_functions();
    assert!(functions.len() > 200, "too few functions were parsed");

    let mut by_module: HashMap<(String, String), Function> = HashMap::new();
    for f in &functions {
        by_module.insert((f.module.clone(), f.name.clone()), f.clone());
    }

    let mut silent = Vec::new();
    let mut used = Vec::new();

    for f in &functions {
        if !f.public {
            continue;
        }
        let mut visited = HashSet::new();
        if let Verdict::Unsafe = verdict(&f.module, &f.name, &by_module, &mut visited) {
            let at = format!("{}::{}", f.module, f.name);
            match TOLERATED.iter().find(|(known, _)| *known == at) {
                Some((known, _)) => used.push(*known),
                None => silent.push(format!(
                    "src/{}.rs:{} — {at}",
                    f.module.replace("::", "/"),
                    f.line
                )),
            }
        }
    }

    assert!(
        silent.is_empty(),
        "these answer `not_found` and nothing in their own body, or in a \
         private function they call first, asks the host before they do:\n  {}\n\
         Ask before the miss, the way `credit::refund_to_credit` and `cart::open` \
         do — or name it in TOLERATED with the reason.",
        silent.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these ask now, or are gone; take them out of TOLERATED: {stale:?}"
    );
}

#[test]
fn a_function_that_answers_without_asking_is_caught() {
    let source = "\
pub async fn set_customer(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<Cart> {
    open(tx, ctx, id, Action::Write).await?;
    sqlx::query_as::<_, Cart>(\"update cart ..\")
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found(\"cart\"))
}

async fn open(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId, action: Action) -> Result<Cart> {
    let cart = sqlx::query_as::<_, Cart>(\"select ..\")
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found(\"cart\"))?;
    let _: Permit = ctx.permit(action, Resource::Cart { id, customer: None })?;
    Ok(cart)
}
";
    let functions = functions_in("cart", source);
    let mut by_module = HashMap::new();
    for f in &functions {
        by_module.insert((f.module.clone(), f.name.clone()), f.clone());
    }

    let mut visited = HashSet::new();
    assert!(
        matches!(
            verdict("cart", "set_customer", &by_module, &mut visited),
            Verdict::Unsafe
        ),
        "a function whose gate reads the row before asking must be caught, \
         not waved through because it asks somewhere"
    );
}

#[test]
fn a_function_that_asks_first_is_not_flagged() {
    let source = "\
pub async fn by_email(tx: &mut Tx<'_>, ctx: &Ctx<'_>, email: &str) -> Result<Customer> {
    let _: Permit = ctx.permit(Action::View, Resource::Customer { id: None })?;
    sqlx::query_as::<_, Customer>(\"select ..\")
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found(\"customer\"))
}
";
    let functions = functions_in("customer", source);
    let mut by_module = HashMap::new();
    for f in &functions {
        by_module.insert((f.module.clone(), f.name.clone()), f.clone());
    }

    let mut visited = HashSet::new();
    assert!(
        matches!(
            verdict("customer", "by_email", &by_module, &mut visited),
            Verdict::Safe
        ),
        "asking before the query is answered must pass"
    );
}

#[test]
fn a_function_that_delegates_to_a_gate_asking_first_is_not_flagged() {
    let source = "\
pub async fn update_line(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId) -> Result<Cart> {
    open(tx, ctx, id, Action::Write).await?;
    sqlx::query_as::<_, LineItem>(\"update ..\")
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| Error::not_found(\"line item\"))
}

async fn open(tx: &mut Tx<'_>, ctx: &Ctx<'_>, id: CartId, action: Action) -> Result<Cart> {
    let cart = sqlx::query_as::<_, Cart>(\"select ..\")
        .fetch_optional(&mut **tx)
        .await?;
    let Some(cart) = cart else {
        let _: Permit = ctx.permit(action, Resource::Cart { id, customer: None })?;
        return Err(Error::not_found(\"cart\"));
    };
    let _: Permit = ctx.permit(action, Resource::Cart { id, customer: cart.customer_id })?;
    Ok(cart)
}
";
    let functions = functions_in("cart", source);
    let mut by_module = HashMap::new();
    for f in &functions {
        by_module.insert((f.module.clone(), f.name.clone()), f.clone());
    }

    let mut visited = HashSet::new();
    assert!(
        matches!(
            verdict("cart", "update_line", &by_module, &mut visited),
            Verdict::Safe
        ),
        "a gate called first that itself asks before answering not_found \
         must clear the function that calls it"
    );
}
