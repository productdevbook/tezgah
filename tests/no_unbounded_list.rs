//! "Listing returns a `Page<T>` with a cursor. There is no unbounded list."
//!
//! `SCHEMA.md` says it, so something had better check it. This reads the
//! crate's own source, finds every public function handing back a `Vec`, and
//! insists each one is bounded by something a reader can point at.
//!
//! Three ways to be bounded, in order of preference:
//!
//! 1. take a `Paging` and hand back a `Page<T>`;
//! 2. cap the query with a named `MAX_*` constant, the way `catalogue.rs` does
//!    with `MAX_ATTACHED`, where the list is configuration rather than a
//!    customer's data;
//! 3. touch no database at all, so the length is whatever the caller passed in.
//!
//! Anything else has to be named in `TOLERATED` below with a reason. That list
//! is a ratchet, not a permission: it is seeded with what was already unbounded
//! when the check was written (tracked in issue #52) and nothing may be added
//! to it. Emptying it closes that issue.

use std::path::Path;

/// Unbounded today, tracked in #52. Do not add to this list.
const TOLERATED: [(&str, &str); 17] = [
    ("cart::lines", "grows with the line items in one cart"),
    ("cart::expire", "sweeps every expired cart in one go"),
    ("customer::group_ids", "grows with a customer's groups"),
    ("fulfilment::providers", "grows with configured providers"),
    ("fulfilment::service_zones", "grows with a set's zones"),
    (
        "fulfilment::zones_for",
        "grows with the zones an address hits",
    ),
    (
        "fulfilment::options_for",
        "grows with the options an address hits",
    ),
    ("fulfilment::labels", "grows with a fulfilment's labels"),
    (
        "inventory::inventory_items_for_variant",
        "grows with the items behind a variant",
    ),
    (
        "inventory::reservations_for_line_item",
        "grows with a line's reservations",
    ),
    ("payment::providers", "grows with configured providers"),
    ("pricing::price_rules", "grows with a price's rules"),
    ("promotion::apply", "grows with the cart it is applied to"),
    ("store::currencies", "grows with configured currencies"),
    ("tax::tax_rate_rules", "grows with a rate's rules"),
    ("tax::rates_for", "grows with the regions an address hits"),
    ("tax::calculate", "grows with the lines handed in"),
];

struct Function {
    at: String,
    path: String,
    signature: String,
    body: String,
}

/// Every `pub fn` and `pub async fn` in a file, signature and body apart.
fn functions(module: &str, source: &str) -> Vec<Function> {
    let bytes: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut offset = 0;

    while let Some(hit) = source[offset..].find("pub ") {
        let start = offset + hit;
        offset = start + 4;

        let head = &source[start..];
        let name_at = if let Some(rest) = head.strip_prefix("pub async fn ") {
            source.len() - rest.len()
        } else if let Some(rest) = head.strip_prefix("pub fn ") {
            source.len() - rest.len()
        } else {
            continue;
        };
        // Only an item, never `pub` inside a string or a longer word.
        if source[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_alphanumeric() || c == '_')
        {
            continue;
        }

        let name: String = source[name_at..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();

        let chars_before = source[..start].chars().count();
        let mut depth = 0i32;
        let mut i = chars_before;
        let mut body_start = None;
        while i < bytes.len() {
            match bytes[i] {
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
            continue;
        };

        let mut depth = 0i32;
        let mut j = body_start;
        while j < bytes.len() {
            match bytes[j] {
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

        let signature: String = bytes[chars_before..body_start].iter().collect();
        let body: String = bytes[body_start..j.min(bytes.len())].iter().collect();
        let line = source[..start].matches('\n').count() + 1;

        found.push(Function {
            at: format!("{module}::{name}"),
            path: format!("src/{}.rs:{line}", module.replace("::", "/")),
            signature,
            body,
        });
    }

    found
}

/// Every `.rs` under `src/`, keyed by its module path, subdirectories included.
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

fn returns_a_vec(signature: &str) -> bool {
    signature
        .split_once("->")
        .is_some_and(|(_, ret)| ret.contains("Vec<"))
}

#[test]
fn no_public_function_hands_back_a_list_with_no_bound_on_it() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    let mut sources = Vec::new();
    collect(&dir, &dir, &mut sources);
    sources.sort_by(|a: &(String, String), b| a.0.cmp(&b.0));

    assert!(!sources.is_empty(), "no source files were scanned");

    let mut unbounded = Vec::new();
    let mut used = Vec::new();

    for (module, source) in &sources {
        for function in functions(module, source) {
            if !returns_a_vec(&function.signature) {
                continue;
            }
            if function.signature.contains("Paging")
                || !function.body.contains("sqlx::")
                || function.body.contains("MAX_")
            {
                continue;
            }
            match TOLERATED.iter().find(|(name, _)| *name == function.at) {
                Some((name, _)) => used.push(*name),
                None => unbounded.push(format!("{} — {}", function.path, function.at)),
            }
        }
    }

    assert!(
        unbounded.is_empty(),
        "these public functions read rows into a Vec with no page, no cap and no reason:\n  {}\n\
         Give them a Paging and a Page<T>, or a named MAX_* ceiling.",
        unbounded.join("\n  ")
    );

    let stale: Vec<&str> = TOLERATED
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !used.contains(name))
        .collect();
    assert!(
        stale.is_empty(),
        "these are bounded now, or gone; take them out of TOLERATED: {stale:?}"
    );
}
