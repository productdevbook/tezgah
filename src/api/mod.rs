//! The two surfaces: what a storefront may ask, and what a back office may.
//!
//! tezgah does not choose an HTTP framework for its host. A route here is a
//! plain async function over a transaction and a context, taking a typed input
//! and returning a typed view — the same shape as a domain call, with the
//! serialisation decided and the permission declared.
//!
//! What that buys: a host on axum, actix or anything else wires these up
//! itself, the OpenAPI document is generated from [`ROUTES`] rather than
//! written beside it, and the permission matrix test reads the same table the
//! router does, so a route cannot be open to somebody the table says it is not.
//!
//! # Views are not rows
//!
//! A row is what the database holds; a view is what leaves the building. They
//! are separate types on purpose. A column added to a table should not appear
//! in an API response because nobody stopped it, and `scope` must never appear
//! at all.

pub mod admin;
pub mod admin_catalogue;
pub mod admin_rest;
pub mod openapi;
pub mod store;

use crate::ports::Action;

/// Which surface a route belongs to. The same resource is usually reachable on
/// both and answers differently: a shopper sees a published product, an
/// operator sees the draft beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Surface {
    /// Reached by a shopper, with a publishable key.
    Store,
    /// Reached by a back office.
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Method {
    Get,
    Post,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }
}

/// One endpoint, described well enough to route it, document it and test who
/// may reach it — without the handler being involved.
#[derive(Debug, Clone, Copy)]
pub struct Route {
    pub surface: Surface,
    pub method: Method,
    /// With `{}` around parameters: `/products/{id}/variants`.
    pub path: &'static str,
    /// What the handler asks of the host's authorizer before it reads a row.
    pub action: Action,
    /// The domain this belongs to, which is also its OpenAPI tag.
    pub domain: &'static str,
    /// One line, used as the OpenAPI summary.
    pub summary: &'static str,
}

/// Every endpoint tezgah serves.
///
/// The router, the OpenAPI document and the permission matrix test all read
/// this, so a route that exists in one and not the others cannot happen.
pub fn routes() -> Vec<Route> {
    let mut all = Vec::with_capacity(
        store::ROUTES.len()
            + admin::ROUTES.len()
            + admin_catalogue::ROUTES.len()
            + admin_rest::ROUTES.len(),
    );
    all.extend_from_slice(store::ROUTES);
    all.extend_from_slice(admin::ROUTES);
    all.extend_from_slice(admin_catalogue::ROUTES);
    all.extend_from_slice(admin_rest::ROUTES);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_two_routes_answer_the_same_call() {
        let mut seen = std::collections::BTreeSet::new();
        for route in routes() {
            let key = (route.surface, route.method, route.path);
            assert!(
                seen.insert(key),
                "{} {} is declared twice on the same surface",
                route.method.as_str(),
                route.path
            );
        }
    }

    #[test]
    fn a_path_parameter_is_written_the_one_way() {
        for route in routes() {
            assert!(
                !route.path.contains(':') && !route.path.contains('<'),
                "{} uses a framework's parameter syntax; use {{name}}",
                route.path
            );
            assert!(
                route.path.starts_with('/') && !route.path.ends_with('/'),
                "{} should start with a slash and not end with one",
                route.path
            );
        }
    }

    #[test]
    fn a_reading_route_does_not_ask_for_a_writing_permission() {
        for route in routes() {
            if route.method == Method::Get {
                assert_eq!(
                    route.action,
                    Action::View,
                    "{} reads but asks for {:?}",
                    route.path,
                    route.action
                );
            }
        }
    }
}
