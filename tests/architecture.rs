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

fn graph() -> BTreeMap<String, BTreeSet<String>> {
    let names = modules();
    let mut edges = BTreeMap::new();

    for module in &names {
        let text = source(module);
        let mut used = BTreeSet::new();
        for other in &names {
            if other == module {
                continue;
            }
            // `crate::other::` or `use crate::other;`
            let path = format!("crate::{other}");
            if text.contains(&format!("{path}::")) || text.contains(&format!("{path};")) {
                used.insert(other.clone());
            }
        }
        edges.insert(module.clone(), used);
    }

    edges
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
                "cart",
                "catalogue",
                "checkout",
                "customer",
                "fulfilment",
                "inventory",
                "order",
                "payment",
                "pricing",
                "promotion",
                "tax",
            ][..],
        ),
        ("order", &["cart", "inventory", "payment", "promotion"][..]),
        (
            "checkout",
            &["cart", "inventory", "order", "payment", "promotion"][..],
        ),
        ("providers", &["payment"][..]),
    ]);

    let mut wrong = Vec::new();
    for (module, used) in &edges {
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

    assert!(
        wrong.is_empty(),
        "a domain reached for another that was not declared. If the dependency is \
         right, add it to `allowed` in this test and say why: {wrong:?}"
    );
}
