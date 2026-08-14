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
    match serde_json::to_string_pretty(&tezgah::api::openapi::document()) {
        Ok(text) => text,
        Err(error) => panic!("the document did not serialise: {error}"),
    }
}

#[test]
fn the_document_matches_what_was_agreed() {
    let path = snapshot();
    let fresh = generated();

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
