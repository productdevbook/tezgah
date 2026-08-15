//! The shape of the crate, asserted rather than agreed.
//!
//! tezgah is one crate on purpose: splitting commerce domains into separate
//! crates invites a cycle the moment two of them need each other, and the usual
//! escape — a shared types crate — becomes the place everything ends up.
//!
//! The benefit a workspace would have bought is the boundary being enforced
//! instead of remembered. That is what this file buys instead, for the price of
//! reading the source.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

/// What every domain may use and what may use nothing but itself.
const KERNEL: &[&str] = &["error", "id", "money", "page", "ports", "workflow"];

/// Modules a domain may reach for besides the kernel. `store` holds the
/// currency exponent every amount is rounded by, so everything is allowed to
/// ask it.
const SHARED: &[&str] = &["store"];

fn modules() -> Vec<String> {
    let mut found = Vec::new();
    for entry in fs::read_dir("src").expect("src to be readable") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("a file name")
            .to_owned();

        if name == "lib" || name == "main" {
            continue;
        }
        if path.is_dir() || path.extension().is_some_and(|e| e == "rs") {
            found.push(name);
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Reads a module's source with doc comments removed.
///
/// A `[link](crate::other)` in documentation is not a dependency, and counting
/// one produced a cycle between `ports` and `error` that does not exist.
fn source(module: &str) -> String {
    let file = Path::new("src").join(format!("{module}.rs"));
    let mut text = String::new();

    if file.exists() {
        text.push_str(&fs::read_to_string(&file).expect("a module to be readable"));
    } else {
        let dir = Path::new("src").join(module);
        for entry in fs::read_dir(&dir).expect("a module directory to be readable") {
            let path = entry.expect("a directory entry").path();
            if path.extension().is_some_and(|e| e == "rs") {
                text.push_str(&fs::read_to_string(&path).expect("a module file to be readable"));
            }
        }
    }

    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("//!") && !trimmed.starts_with("///") && !trimmed.starts_with("//")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word characters an identifier is made of.
fn read_ident(s: &str) -> &str {
    let end = s
        .char_indices()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(s.len(), |(i, _)| i);
    &s[..end]
}

/// The index of the `}` matching the `{` at `s`'s start, counting nesting.
fn matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Splits a brace group's contents on its top-level commas, so a nested
/// group's own commas — `credit::{Kind, Reason}` inside a bigger group —
/// are not mistaken for separators between siblings.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 => {
                items.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    items.push(&s[start..]);
    items
}

/// The module named right after one `crate::` — `crate::name::rest`,
/// `crate::name;` and, for a braced group, every direct child:
/// `crate::{a, b::{c, d}}` names `a` and `b`, not `c` or `d`, because a
/// nested path's own tail is not a further module of `crate` itself.
fn modules_after(rest: &str) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(stripped) = rest.strip_prefix('{') {
        let Some(close) = matching_brace(rest) else {
            return found;
        };
        let inner = &stripped[..close - 1];
        for item in split_top_level(inner) {
            let ident = read_ident(item.trim_start());
            if !ident.is_empty() && ident != "self" {
                found.push(ident.to_owned());
            }
        }
    } else {
        let ident = read_ident(rest);
        if !ident.is_empty() && ident != "self" {
            found.push(ident.to_owned());
        }
    }
    found
}

/// Every module `text` reaches `crate::` into, restricted to `names` and with
/// self-references dropped. Split out of [`graph`] so a test can feed it a
/// literal string and ask what the latch would have seen, with no file
/// written and no `src/` involved.
fn used_in(names: &[String], module: &str, text: &str) -> BTreeSet<String> {
    let mut used = BTreeSet::new();
    for (start, _) in text.match_indices("crate::") {
        let rest = &text[start + "crate::".len()..];
        for found in modules_after(rest) {
            if names.contains(&found) && found != module {
                used.insert(found);
            }
        }
    }
    used
}

fn graph() -> BTreeMap<String, BTreeSet<String>> {
    let names = modules();
    let mut edges = BTreeMap::new();

    for module in &names {
        let text = source(module);
        edges.insert(module.clone(), used_in(&names, module, &text));
    }

    edges
}

/// Every edge in `edges` that `allowed` does not admit — the kernel and
/// `SHARED` aside. Split out of the test below so a synthetic graph can be
/// checked with the exact rule the real one is, rather than a restatement of
/// it.
fn violations(
    edges: &BTreeMap<String, BTreeSet<String>>,
    allowed: &BTreeMap<&str, &[&str]>,
) -> Vec<String> {
    let mut wrong = Vec::new();
    for (module, used) in edges {
        if KERNEL.contains(&module.as_str()) {
            continue;
        }
        for other in used {
            let fine = KERNEL.contains(&other.as_str())
                || SHARED.contains(&other.as_str())
                || allowed
                    .get(module.as_str())
                    .is_some_and(|list| list.contains(&other.as_str()));
            if !fine {
                wrong.push(format!("{module} -> {other}"));
            }
        }
    }
    wrong
}

#[test]
fn no_module_depends_on_itself_through_others() {
    let edges = graph();
    let mut cycles = Vec::new();

    // A cycle exists iff some module is reachable from itself.
    for start in edges.keys() {
        let mut seen = BTreeSet::new();
        let mut queue: VecDeque<&String> = edges[start].iter().collect();

        while let Some(next) = queue.pop_front() {
            if next == start {
                cycles.push(start.clone());
                break;
            }
            if !seen.insert(next.clone()) {
                continue;
            }
            if let Some(further) = edges.get(next) {
                queue.extend(further.iter());
            }
        }
    }

    assert!(
        cycles.is_empty(),
        "these modules can reach themselves, so the crate can no longer be split \
         along its own seams: {cycles:?}"
    );
}

#[test]
fn the_kernel_depends_on_nothing_above_it() {
    let edges = graph();
    let mut wrong = Vec::new();

    for module in KERNEL {
        let Some(used) = edges.get(*module) else {
            continue;
        };
        for other in used {
            if !KERNEL.contains(&other.as_str()) {
                wrong.push(format!("{module} -> {other}"));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "the kernel reached upward into a domain, which is what makes it no longer \
         a kernel: {wrong:?}"
    );
}

#[test]
fn a_domain_reaches_only_for_the_kernel_and_what_is_declared_shared() {
    let edges = graph();

    // Written down rather than inferred: each is a domain that genuinely builds
    // on another, and a new one should be a decision somebody makes on purpose.
    let allowed: BTreeMap<&str, &[&str]> = BTreeMap::from([
        // The surfaces are where the domains are finally allowed to meet: an
        // API is by definition the outside edge, and nothing may depend on it.
        (
            "api",
            &[
                "batch",
                "cart",
                "catalogue",
                "checkout",
                "customer",
                "fulfilment",
                "inventory",
                "order",
                "payment",
                "pricing",
                "credit",
                "digital",
                "promotion",
                "settlement",
                "subscription",
                "tax",
            ][..],
        ),
        // A variant says whether buying it can be walked away from, and the
        // list of exemptions is the order module's. The arrow only goes this
        // way: an order line records the answer that held on the day of the
        // sale, and never asks the catalogue again.
        ("catalogue", &["order"][..]),
        // An import writes products, their prices and their stock, which is what
        // makes it an import rather than three of them.
        ("batch", &["catalogue", "inventory", "pricing"][..]),
        (
            "order",
            &["cart", "fulfilment", "inventory", "payment", "promotion"][..],
        ),
        // A parcel leaving is the moment stock leaves, so the module that writes
        // the parcel is the one that has to move the count. The arrow only goes
        // this way: `inventory` knows nothing of parcels, and the day it wants
        // to, the shared part moves to the kernel rather than the arrow
        // reversing.
        ("fulfilment", &["inventory"][..]),
        // checkout::run reads a sold line's facts — withdrawal exemption
        // among them — before it lets an order be placed, so it reaches
        // catalogue for the same reason cart's totals do. Nothing reaches
        // back: catalogue's only edge is to order, never to checkout.
        (
            "checkout",
            &[
                "cart",
                "catalogue",
                "credit",
                "inventory",
                "order",
                "payment",
                "promotion",
            ][..],
        ),
        // A gift card is money owed against an order and carried on a payment
        // collection, so it reaches both. Neither reaches back: an order knows
        // it has a credit line, not what minted it.
        ("credit", &["order", "payment"][..]),
        // A file is catalogue until it is bought and order state afterwards, so
        // it reaches both and neither reaches back: the day `order` wants to
        // know what a line entitled somebody to, it asks through a surface.
        ("digital", &["catalogue", "order"][..]),
        // A contract resolves its own price, reserves its own stock, writes its
        // own order and charges a stored instrument, so it reaches most of the
        // shop. Nothing reaches back: the day `order` wants to know which
        // contract produced it, it asks through a surface, because the arrow
        // reversing is the cycle.
        (
            "subscription",
            &["inventory", "order", "payment", "pricing", "tax"][..],
        ),
        ("providers", &["payment"][..]),
        // The top of the graph: everything that must happen because money
        // arrived, so it reaches for the domains that decide what that is.
        // `fulfilment` is here rather than `digital` reaching for it: a
        // digital line's `order_item` counters are `fulfilment`'s to write,
        // the way a parcel's are, and settlement is the one place already
        // holding both `digital`'s answer and the order it belongs to.
        // Nothing reaches back — see `no_module_depends_on_settlement` below.
        (
            "settlement",
            &[
                "credit",
                "digital",
                "fulfilment",
                "order",
                "payment",
                "subscription",
            ][..],
        ),
        // promotion::apply reads a cart's lines and bounds them by the ceiling
        // cart owns. This is the edge that would become a cycle the day cart
        // asks promotion what a line is worth, so if that is ever wanted, the
        // shared part moves to the kernel rather than the arrow being reversed.
        ("promotion", &["cart"][..]),
    ]);

    let wrong = violations(&edges, &allowed);

    assert!(
        wrong.is_empty(),
        "a domain reached for another that was not declared. If the dependency is \
         right, add it to `allowed` in this test and say why: {wrong:?}"
    );
}

#[test]
fn nothing_depends_on_settlement() {
    let edges = graph();
    let mut wrong = Vec::new();

    for (module, used) in &edges {
        // `api` is the surface a route lives on, and calling settlement from a
        // route is the whole point — see `README.md`'s port table.
        if module == "settlement" || module == "api" {
            continue;
        }
        if used.contains("settlement") {
            wrong.push(module.clone());
        }
    }

    assert!(
        wrong.is_empty(),
        "settlement is the top of the graph — nothing calls it but a route, and no \
         domain reaches for it: {wrong:?}"
    );
}

// ---------------------------------------------------------------------------
// The latch's own parsing — no file written, no `src/` read.
// ---------------------------------------------------------------------------

#[test]
fn a_single_module_import_is_seen() {
    let names = vec!["order".to_string()];
    assert_eq!(
        used_in(&names, "cart", "use crate::order::NewOrder;\n"),
        BTreeSet::from(["order".to_string()])
    );
    assert_eq!(
        used_in(&names, "cart", "use crate::order;\n"),
        BTreeSet::from(["order".to_string()])
    );
}

/// The form #130 was written about: `use crate::{a, b};` names both `a` and
/// `b`, even though neither is followed by `::` or `;`.
#[test]
fn a_braced_group_of_bare_modules_is_seen() {
    let names = vec![
        "cart".to_string(),
        "catalogue".to_string(),
        "credit".to_string(),
    ];
    let used = used_in(
        &names,
        "checkout",
        "use crate::{cart, catalogue, credit, inventory, promotion};\n",
    );
    assert_eq!(
        used,
        BTreeSet::from([
            "cart".to_string(),
            "catalogue".to_string(),
            "credit".to_string()
        ])
    );
}

/// A group can mix a bare module with one carrying its own path, and nest
/// another group inside — `checkout.rs` and `api/store.rs` both do. Only the
/// direct children name a module; `Kind` and `Reason` are items of `credit`,
/// not further modules of `crate`.
#[test]
fn a_nested_and_multiline_group_is_seen() {
    let names = vec![
        "cart".to_string(),
        "credit".to_string(),
        "order".to_string(),
    ];
    let text = "use crate::{\n    cart::{CartTotals, TotalsLine},\n    credit::{Kind, Reason},\n    order,\n};\n";
    let used = used_in(&names, "checkout", text);
    assert_eq!(
        used,
        BTreeSet::from([
            "cart".to_string(),
            "credit".to_string(),
            "order".to_string()
        ])
    );
}

/// A doc link is not an import: `[link](crate::other)` must not be counted,
/// which is [`source`]'s job upstream of `used_in` — this only guards that
/// `used_in` itself does not need the comment stripped to behave, so a
/// caller who forgets to strip is not silently safe.
#[test]
fn a_self_import_inside_a_group_is_not_a_self_edge() {
    let names = vec!["workflow".to_string(), "error".to_string()];
    let used = used_in(
        &names,
        "workflow",
        "use crate::{error::{Error, Result}, workflow::run};\n",
    );
    assert_eq!(used, BTreeSet::from(["error".to_string()]));
}

/// The point of #130: a forbidden edge written with braces is not merely
/// parsed, it is what turns `a_domain_reaches_only_for_the_kernel_and_what_is_declared_shared`
/// red. This runs the same `violations` the real test does, against a
/// synthetic graph built only from `used_in` on a literal string, so nothing
/// here depends on `src/` still containing the offending line.
#[test]
fn a_forbidden_edge_written_as_a_braced_import_fails_the_domain_check() {
    let names = vec!["payment".to_string(), "credit".to_string()];

    let mut edges = BTreeMap::new();
    edges.insert(
        "payment".to_string(),
        used_in(&names, "payment", "use crate::{credit};\n"),
    );
    edges.insert("credit".to_string(), BTreeSet::new());

    // Nothing is declared for `payment` to reach — the real allowed list
    // does not grant it `credit` either, for the reason `README.md` gives:
    // teaching `payment` about `credit` is the cycle `settlement` exists to
    // avoid.
    let allowed: BTreeMap<&str, &[&str]> = BTreeMap::new();

    assert_eq!(
        violations(&edges, &allowed),
        vec!["payment -> credit".to_string()],
        "a forbidden edge written with a braced import did not turn the latch red"
    );
}
