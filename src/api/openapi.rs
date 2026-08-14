//! The OpenAPI document, generated from [`routes`](super::routes).
//!
//! Written by hand it would drift the first time a route moved, and the drift
//! would be invisible until a client generated from it failed against the
//! server. Generated, the document cannot say anything the table does not.
//!
//! What it does not carry is schemas for bodies and views: those live on the
//! Rust types, and deriving them is a separate piece of work. Everything the
//! table knows — the path, the method, the summary, the tag, the parameters and
//! who may call it — is here and is exact.

use serde_json::{Map, Value, json};

use super::{Method, Route, Surface, routes};

/// A storefront's key, which pins it to its sales channels.
const STORE_SCHEME: &str = "publishableKey";
/// A back office's token. tezgah does not issue it; the host's authorizer
/// reads it and answers.
const ADMIN_SCHEME: &str = "adminBearer";

fn scheme(surface: Surface) -> &'static str {
    match surface {
        Surface::Store => STORE_SCHEME,
        Surface::Admin => ADMIN_SCHEME,
    }
}

fn verb(method: Method) -> &'static str {
    match method {
        Method::Get => "get",
        Method::Post => "post",
        Method::Patch => "patch",
        Method::Delete => "delete",
    }
}

/// The names between braces, in the order they appear.
fn parameters(path: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = path;

    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                found.push(&after[..close]);
                rest = &after[close + 1..];
            }
            None => break,
        }
    }

    found
}

/// `GET /admin/customers/{id}` becomes `getAdminCustomersById`, so two routes
/// on one path do not collide and a generated client reads.
fn operation_id(route: &Route) -> String {
    let mut name = String::from(verb(route.method));

    for segment in route.path.split('/').filter(|s| !s.is_empty()) {
        let (prefix, word) = match segment.strip_prefix('{') {
            Some(inner) => ("By", inner.trim_end_matches('}')),
            None => ("", segment),
        };
        name.push_str(prefix);
        for part in word.split(['-', '_']).filter(|p| !p.is_empty()) {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                name.extend(first.to_uppercase());
                name.push_str(chars.as_str());
            }
        }
    }

    name
}

fn operation(route: &Route) -> Value {
    let parameters: Vec<Value> = parameters(route.path)
        .into_iter()
        .map(|name| {
            json!({
                "name": name,
                "in": "path",
                "required": true,
                "schema": { "type": "string" },
            })
        })
        .collect();

    json!({
        "operationId": operation_id(route),
        "summary": route.summary,
        "tags": [route.domain],
        "parameters": parameters,
        "security": [{ scheme(route.surface): [] }],
        "x-tezgah-permission": format!("{:?}", route.action).to_lowercase(),
        "responses": {
            "200": { "description": "The call succeeded." },
            "400": { "description": "The request was not well formed." },
            "403": { "description": "The host's authorizer refused." },
            "404": { "description": "No such thing, or none this caller may see." },
        },
    })
}

/// Every tag mentioned by a route, once, in order.
fn tags() -> Vec<Value> {
    let mut names: Vec<&'static str> = routes().iter().map(|route| route.domain).collect();
    names.sort_unstable();
    names.dedup();
    names
        .into_iter()
        .map(|name| json!({ "name": name }))
        .collect()
}

/// The whole document. Deterministic: `serde_json` orders object keys, and the
/// route table is read in the order it is declared.
pub fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();

    for route in routes() {
        let entry = paths
            .entry(route.path.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));

        if let Some(object) = entry.as_object_mut() {
            object.insert(verb(route.method).to_owned(), operation(&route));
        }
    }

    json!({
        "openapi": "3.1.0",
        "info": {
            "title": env!("CARGO_PKG_NAME"),
            "version": env!("CARGO_PKG_VERSION"),
            "description": env!("CARGO_PKG_DESCRIPTION"),
            "license": {
                "name": "MIT",
                "identifier": "MIT",
            },
        },
        "tags": tags(),
        "paths": paths,
        "components": {
            "securitySchemes": {
                STORE_SCHEME: {
                    "type": "apiKey",
                    "in": "header",
                    "name": "x-publishable-api-key",
                    "description": "A storefront's publishable key, which pins it to its sales channels.",
                },
                ADMIN_SCHEME: {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "A back office's token. tezgah does not issue it; the host's authorizer reads it.",
                },
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_parameter_is_declared_as_one() {
        let found = parameters("/admin/customers/{id}/addresses/{address_id}");
        assert_eq!(found, vec!["id", "address_id"]);
        assert!(parameters("/store/products").is_empty());
    }

    #[test]
    fn every_route_reaches_the_document() {
        let document = document();
        let paths = document
            .get("paths")
            .and_then(Value::as_object)
            .map(|object| object.len())
            .unwrap_or_default();

        let mut distinct: Vec<&'static str> = routes().iter().map(|route| route.path).collect();
        distinct.sort_unstable();
        distinct.dedup();

        assert_eq!(paths, distinct.len());
    }

    #[test]
    fn every_operation_says_who_may_call_it() {
        let document = document();
        let empty = Map::new();
        let paths = document
            .get("paths")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        assert!(!paths.is_empty(), "the document has no paths in it");

        for (path, item) in paths {
            let Some(item) = item.as_object() else {
                continue;
            };
            for (method, operation) in item {
                assert!(
                    operation
                        .get("security")
                        .is_some_and(|s| !s.as_array().map(Vec::is_empty).unwrap_or(true)),
                    "{method} {path} is documented as open to anybody"
                );
            }
        }
    }
}
