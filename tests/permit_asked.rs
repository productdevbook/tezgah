//! "Every public function that reaches the database asked the host first."
//!
//! `Permit` is not a key the compiler makes you carry: no function in the
//! crate takes one as a parameter, and issue #79 says so plainly. What is
//! true is weaker and worth stating exactly — every public function whose
//! body runs a query either calls `ctx.permit(..)` itself or reaches the
//! database through a function in this crate that does. This test reads
//! `src/` and holds that line, the way `tests/no_unbounded_list.rs` holds the
//! paging rule.
//!
//! What it proves: a new public function that runs a query and asks nobody
//! fails CI rather than review.
//!
//! What it does not prove: that the permit named the right `Action` or the
//! right `Resource`, or that an authorizer answering yes to everything is
//! being asked anything worth asking. Calls are matched by name, so a private
//! helper that asks makes every same-named path count as asking.
//!
//! Anything else is named in `TOLERATED` with a reason. The list may shrink.

use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Functions that run a query without a permit anywhere beneath them, each
/// with the reason it is not a hole. Adding to this is not a fix.
const TOLERATED: [(&str, &str); 3] = [
    (
        "payment::recompute",
        "derives a collection's amounts from rows already written; `pub(crate)` \
         so credit can call it, and every in-crate caller asked before reaching it",
    ),
    (
        "workflow::recover",
        "the runner putting back leases its own dead workers held; there is no \
         actor to ask about",
    ),
    (
        "workflow::extend",
        "a running step extending its own lease; takes no Ctx, so there is \
         nobody to ask",
    ),
];

const ASKS: &str = "ctx.permit(";

struct Function {
    name: String,
    module: String,
    line: usize,
    public: bool,
    body: String,
}

fn ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every `fn` in a file with a body, private ones included.
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
        let public = head.trim_start().starts_with("pub");
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

/// Calls a body makes, as `(module, name)`: bare `helper(..)` resolves in the
/// same module, `other::helper(..)` in whichever module ends with that segment.
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

#[test]
fn every_public_function_that_reaches_the_database_asked_first() {
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

    let mut asks: HashMap<(String, String), bool> = HashMap::new();
    for function in &all {
        let entry = asks
            .entry((function.module.clone(), function.name.clone()))
            .or_insert(false);
        *entry = *entry || function.body.contains(ASKS);
    }

    // Asking is inherited: a function reaching the database only through one
    // that asked has asked.
    loop {
        let mut settled = true;
        for function in &all {
            let key = (function.module.clone(), function.name.clone());
            if asks.get(&key).copied().unwrap_or(false) {
                continue;
            }
            if callees(&function.module, &function.body, &modules)
                .iter()
                .any(|callee| asks.get(callee).copied().unwrap_or(false))
            {
                asks.insert(key, true);
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
        if !function.public || !function.body.contains("sqlx::") {
            continue;
        }
        let key = (function.module.clone(), function.name.clone());
        if asks.get(&key).copied().unwrap_or(false) {
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
        "these public functions run a query without a `ctx.permit(..)` above them:\n  {}\n\
         Ask the host — `let _: Permit = ctx.permit(action, resource)?;` — or reach \
         the rows through a function that does.",
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
