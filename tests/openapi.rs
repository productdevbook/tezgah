//! The OpenAPI document, snapshotted.
//!
//! The document is generated from the route table, so this does not check that
//! the generator works — the unit tests beside it do. It checks that the
//! contract has not moved without somebody saying so: a path renamed, a method
//! added, a permission relaxed, a summary rewritten. All of those are things a
//! client generated from this document depends on, and all of them are silent
//! until a reviewer sees the diff.
//!
//! When the change is intended, regenerate:
//!
//! ```text
//! TEZGAH_UPDATE_SNAPSHOT=1 cargo test --test openapi
//! ```

use std::path::{Path, PathBuf};

fn snapshot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots/openapi.json")
}

fn generated() -> String {
    serde_json::to_string_pretty(&tezgah::api::openapi::document())
        .expect("the document to serialise")
}

#[test]
fn the_document_matches_what_was_agreed() {
    let path = snapshot();
    let fresh = generated();

    // Written every run so CI can hand it back as an artifact: this crate is
    // not built on the machine it is written on, so the snapshot cannot be
    // regenerated there.
    let beside = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/openapi.generated.json");
    if let Some(dir) = beside.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&beside, &fresh);

    if std::env::var_os("TEZGAH_UPDATE_SNAPSHOT").is_some() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).expect("somewhere to write the snapshot");
        }
        std::fs::write(&path, format!("{fresh}\n")).expect("to write the snapshot");
        return;
    }

    let held = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "there is no snapshot at {}. Write one with \
             TEZGAH_UPDATE_SNAPSHOT=1 cargo test --test openapi",
            path.display()
        )
    });

    assert_eq!(
        fresh.trim_end(),
        held.trim_end(),
        "the API contract changed. If that was on purpose, update the snapshot \
         with TEZGAH_UPDATE_SNAPSHOT=1 cargo test --test openapi and put the diff \
         in the pull request; if it was not, the route table moved by accident."
    );
}

/// One name collides across both generators today — an id newtype — and it
/// agrees, the only kind `document()` (see its own comment) trusts to
/// resolve by letting the request definition win. Zero disagree. This is
/// what makes that a build failure instead of an assumption: it fails the
/// day any colliding name's two schemas diverge, whatever the count becomes.
#[test]
fn colliding_names_agree_across_generators() {
    for (name, request, response) in tezgah::api::openapi::schema_collisions() {
        assert_eq!(
            request, response,
            "{name} means two different things depending on direction: \
             document() lets the request definition win on the assumption \
             that a colliding name's schema does not depend on direction. \
             {name}'s two schemas disagree, so that assumption just failed — \
             give it two names in components/schemas instead (one per \
             direction, with a one-line reason for the split), or fix the \
             type so both generators agree."
        );
    }
}

#[test]
fn the_snapshot_is_valid_json_and_says_which_openapi_it_is() {
    let held = std::fs::read_to_string(snapshot()).expect("the snapshot to be readable");
    let parsed: serde_json::Value = serde_json::from_str(&held).expect("the snapshot to be json");

    assert_eq!(
        parsed.get("openapi").and_then(serde_json::Value::as_str),
        Some("3.1.0")
    );
    assert!(
        parsed
            .get("paths")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|paths| !paths.is_empty()),
        "the snapshot documents no paths at all"
    );
}

/// No schema name ends in a digit, because `schemars` puts one there.
///
/// Two Rust types with the same short name get disambiguated by a numeric
/// suffix — `OrderView` and `OrderView2` — and which one gets the suffix is
/// decided by the order the generator walks them in. A client binds to a
/// name; a name that moves when somebody adds a type is a client that reads
/// the wrong shape and says nothing.
///
/// It happened: `admin_order::OrderView` and `store::OrderView` collided, the
/// storefront's narrower type took the plain name, and `app/client/`'s compile-time
/// binding to `OrderView` started describing the wrong one. `#[schemars(rename
/// = "Store…")]` on the three storefront types fixed it — in the document,
/// without renaming anything in Rust.
///
/// Zero today. A new one means two types share a short name and one of them
/// needs a `rename` rather than a suffix nobody chose.
#[test]
fn no_schema_name_was_disambiguated_by_a_number() {
    let document = tezgah::api::openapi::document();
    let suffixed: Vec<&str> = document["components"]["schemas"]
        .as_object()
        .expect("the document has schemas")
        .keys()
        .filter(|name| name.ends_with(|c: char| c.is_ascii_digit()))
        .map(String::as_str)
        .collect();

    assert!(
        suffixed.is_empty(),
        "schemars numbered these because two Rust types share a short name; \
         give one of each pair a #[schemars(rename = \"…\")] instead: {suffixed:?}"
    );
}

/// The document said `"parameters": []` for every route, including the ones
/// whose handlers have taken filters since they were written — so every filter
/// the crate supports was invisible to anybody reading the API rather than the
/// Rust.
///
/// This counts the routes that describe their query string, and the count may
/// only go up. It is a floor rather than an equality so that wiring the next
/// list does not fail this test; it is here at all so that what is wired
/// cannot quietly become none. The floor is under the real number on purpose
/// — an operation id that stops matching a route silently drops its entry,
/// and a floor a few below the truth still catches that.
#[test]
fn the_lists_that_filter_say_what_they_filter_on() {
    let document: serde_json::Value =
        serde_json::from_str(&generated()).expect("the document to parse");

    let paths = document["paths"]
        .as_object()
        .expect("the document to have paths");

    let mut described = 0;
    for methods in paths.values() {
        for operation in methods.as_object().into_iter().flat_map(|m| m.values()) {
            let has_query = operation["parameters"]
                .as_array()
                .is_some_and(|list| list.iter().any(|p| p["in"] == "query"));
            if has_query {
                described += 1;
            }
        }
    }

    assert!(
        described >= 20,
        "only {described} operations describe a query parameter; \
         `QUERIES` in src/api/openapi.rs is what grows this"
    );

    let products = &document["paths"]["/admin/products"]["get"]["parameters"];
    let names: Vec<&str> = products
        .as_array()
        .expect("products to carry parameters")
        .iter()
        .filter(|p| p["in"] == "query")
        .filter_map(|p| p["name"].as_str())
        .collect();

    for wanted in ["after", "limit", "status", "q"] {
        assert!(
            names.contains(&wanted),
            "GET /admin/products takes {wanted} and the document does not say so: {names:?}"
        );
    }
}
