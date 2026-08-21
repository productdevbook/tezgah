//! The number in the documentation is the number the router binds.
//!
//! "bound N of 487" appears in six files, hand-copied, and it was wrong more
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
//! connects, and nothing here sends a request. It does want a Tokio context
//! to exist while it builds its idle pool, though, which is why these are
//! `#[tokio::test]` despite awaiting nothing — `sqlx` panics with "this
//! functionality requires a Tokio context" otherwise.

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

#[tokio::test]
async fn the_readme_says_how_many_routes_are_bound() {
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
#[tokio::test]
async fn nothing_is_bound_that_the_table_does_not_declare() {
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
#[tokio::test]
async fn nothing_is_counted_twice() {
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

/// And the README's list is the router's list, line for line.
///
/// It was a paste of one run's startup log, and it rotted the way a paste
/// does: by the time this was written it named 112 of the 253 routes the
/// binary bound, and the repository's own README said it listed every one.
/// Rendering it from `bound.paths` here makes the paste impossible to leave
/// behind — `cargo nextest run -p tezgah-server bound_count` prints the block
/// to put back.
#[tokio::test]
async fn the_readme_lists_every_route_the_router_binds() {
    let bound = fullest();
    let rendered: Vec<String> = bound
        .paths
        .iter()
        .map(|(method, path)| format!("{method:<6} {path}"))
        .collect();

    let readme = include_str!("../README.md");
    let block = readme
        .split("<!-- routes:begin -->")
        .nth(1)
        .and_then(|rest| rest.split("<!-- routes:end -->").next())
        .expect("README.md to carry a `<!-- routes:begin -->` … `<!-- routes:end -->` block");

    let listed: Vec<String> = block
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .map(str::to_owned)
        .collect();

    assert_eq!(
        listed,
        rendered,
        "the route list in README.md is not what the router binds. Put this \
         between the markers:\n\n```\n{}\n```\n",
        rendered.join("\n")
    );
}
