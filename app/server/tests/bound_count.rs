//! The number in the documentation is the number the router binds.
//!
//! "bound N of 486" appears in six files, hand-copied, and it was wrong more
//! than once: the count drifted every time a batch of routes was bound,
//! because it was arithmetic done in a commit message rather than a thing
//! anybody could check. The last correction was six off.
//!
//! So it is checked. This builds the fullest router — every optional surface
//! configured — counts what it mounted out of `tezgah::api::routes()`, and
//! compares that against what `README.md` says. A route bound without the
//! sentence moving fails here, and so does a sentence moved without a route.
//!
//! No database is dialled: `PgPool::connect_lazy` parses a URL and never
//! connects, and nothing here sends a request.

use std::sync::Arc;

use sqlx::PgPool;
use tezgah::ports::Scope;
use tezgah_server::host::ServerHost;
use tezgah_server::http::{self, AppState};
use uuid::Uuid;

/// Everything optional turned on, because the documented number is what a
/// fully configured shop serves rather than what a bare one does.
fn fullest() -> http::Bound {
    let pool = PgPool::connect_lazy("postgres://example.invalid/tezgah_test_unused")
        .expect("connect_lazy parses the url but never dials it");

    let state = AppState {
        pool,
        host: Arc::new(ServerHost),
        // Checkout stays `None`: building one needs a payment provider and a
        // warehouse, and neither belongs in a test that dials no database. It
        // adds exactly one route — `POST /store/carts/{id}/complete` — so the
        // documented number is this one, and the README says so.
        checkout: None,
        scope: Scope(Uuid::nil()),
        admin_token: Some(Arc::from("test-only-admin-token")),
        has_operators: true,
        webhook_secret: Some(Arc::from("test-only-webhook-secret")),
        mailer: None,
        panel_url: None,
        files: None,
    };

    let (_router, bound) = http::router(state);
    bound
}

#[test]
fn the_readme_says_how_many_routes_are_bound() {
    let bound = fullest();
    let counted = bound.paths.len();

    // A marker rather than prose: the word "bound" appears in that file a
    // dozen times in sentences, and a test reading the first one would be
    // checking whichever paragraph somebody edited last.
    let readme = include_str!("../README.md");
    let claim = readme
        .split("<!-- bound-routes: ")
        .nth(1)
        .and_then(|rest| rest.split(" -->").next())
        .and_then(|number| number.trim().parse::<usize>().ok())
        .expect("README.md to carry a `<!-- bound-routes: N -->` marker");

    assert_eq!(
        claim, counted,
        "the `bound-routes` marker in README.md says {claim} and the router \
         binds {counted}. Whichever moved, the other has to follow — the same \
         number is repeated by hand in ../../README.md, ../README.md, \
         ../client/README.md, ../../GOAL.md and ../../docs/architecture.md, \
         and only this one is checked."
    );
}

/// Every path this binary mounts out of the table is a path the table
/// declares. A typo in a route string binds something nothing describes, and
/// the panel's generated client would never call it.
#[test]
fn nothing_is_bound_that_the_table_does_not_declare() {
    let bound = fullest();
    let declared: Vec<(String, &str)> = tezgah::api::routes()
        .into_iter()
        .map(|route| (route.method.as_str().to_owned(), route.path))
        .collect();

    let strangers: Vec<String> = bound
        .paths
        .iter()
        .filter(|(method, path)| {
            !declared
                .iter()
                .any(|(known, at)| known == method && at == path)
        })
        .map(|(method, path)| format!("{method} {path}"))
        .collect();

    assert!(
        strangers.is_empty(),
        "these are mounted against tezgah's tally and are not in its route \
         table — a typo, or one of this binary's own that belongs in `own`: \
         {strangers:?}"
    );
}

/// And nothing is bound twice. Two entries for one path would inflate the
/// count without serving anything more, which is the quietest way for the
/// number above to become a lie while still matching.
#[test]
fn nothing_is_counted_twice() {
    let bound = fullest();
    let mut seen = std::collections::BTreeSet::new();
    let doubled: Vec<String> = bound
        .paths
        .iter()
        .filter(|entry| !seen.insert(**entry))
        .map(|(method, path)| format!("{method} {path}"))
        .collect();

    assert!(doubled.is_empty(), "counted more than once: {doubled:?}");
}
