//! The OpenAPI document, generated from [`routes`](super::routes).
//!
//! Written by hand it would drift the first time a route moved, and the drift
//! would be invisible until a client generated from it failed against the
//! server. Generated, the document cannot say anything the table does not.
//! What the route table alone gives every operation — its path, method,
//! summary, tag, path parameters and permission — is exact and complete.
//!
//! A body is a second, separate thing. It is not on [`Route`]: the shape of
//! what a handler takes or returns lives on the Rust type it already uses,
//! and `schemars` derives a JSON Schema from that type rather than from a
//! second description of it. [`BODIES`] is the seam between the route table
//! and those types — an operation gets a request or response schema only
//! once it is named there, and most are not yet (tezgah#202 tracks finishing
//! the rest, domain by domain, without changing this mechanism).
//!
//! [`Page<T>`](crate::page::Page) is one schema regardless of `T`: a route
//! whose response is a page overlays the item's own schema onto the shared
//! envelope with `allOf` rather than generating a `Page_of_T` copy per list.

use schemars::generate::SchemaSettings;
use schemars::{JsonSchema, SchemaGenerator};
use serde_json::{Map, Value, json};

use crate::page::Page;

use super::{Method, Route, Surface, payout, routes};

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

/// Where a body schema comes from, before it has become JSON: a request
/// describes how a type is deserialised, a response how it is serialised,
/// and a field of `rust_decimal::Decimal` answers those two questions
/// differently — this is how it ends up a `string` on a response and either
/// a `string` or a `number` on a request.
fn schemas() -> (SchemaGenerator, SchemaGenerator) {
    let settings = SchemaSettings::draft2020_12().with(|s| {
        s.definitions_path = "/components/schemas".into();
        s.meta_schema = None;
    });
    (
        settings.clone().for_deserialize().into_generator(),
        settings.for_serialize().into_generator(),
    )
}

fn schema_of<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    generator.subschema_for::<T>().into()
}

/// Registers the shared `Page` envelope and `T`'s own schema, then overlays
/// the second onto the first's `items` with `allOf` — see the module doc for
/// why this is not simply `generator.subschema_for::<Page<T>>()`.
fn page_of<T: JsonSchema>(generator: &mut SchemaGenerator) -> Value {
    let envelope: Value = generator.subschema_for::<Page<()>>().into();
    let items: Value = generator.subschema_for::<T>().into();
    json!({
        "allOf": [
            envelope,
            { "properties": { "items": { "type": "array", "items": items } } },
        ],
    })
}

type SchemaFn = fn(&mut SchemaGenerator) -> Value;

/// One operation's body schemas, keyed by the [`operation_id`] it belongs to.
struct Body {
    operation_id: &'static str,
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
}

/// The pilot for tezgah#202: one whole domain, request and response bodies
/// both, `Page<T>` used three different ways. The rest of the route table
/// carries no entry yet and documents no body, exactly as it did before this
/// module knew how.
static BODIES: &[Body] = &[
    Body {
        operation_id: "postAdminCommissionRules",
        request: Some(schema_of::<payout::SetCommissionRule>),
        response: Some(schema_of::<payout::CommissionRuleView>),
    },
    Body {
        operation_id: "getAdminCommissionRules",
        request: None,
        response: Some(page_of::<payout::CommissionRuleView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdPayoutLines",
        request: None,
        response: Some(page_of::<payout::PayoutLineView>),
    },
    Body {
        operation_id: "getAdminPayouts",
        request: None,
        response: Some(page_of::<payout::PayoutView>),
    },
    Body {
        operation_id: "postAdminPayouts",
        request: Some(schema_of::<payout::CreatePayout>),
        response: Some(schema_of::<payout::PayoutView>),
    },
    Body {
        operation_id: "getAdminPayoutBalanceByCurrencyCode",
        request: None,
        response: Some(schema_of::<payout::BalanceView>),
    },
];

fn operation(
    route: &Route,
    request_gen: &mut SchemaGenerator,
    response_gen: &mut SchemaGenerator,
) -> Value {
    let id = operation_id(route);
    let body = BODIES.iter().find(|entry| entry.operation_id == id);

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

    let mut responses = json!({
        "200": { "description": "The call succeeded." },
        "400": { "description": "The request was not well formed." },
        "403": { "description": "The host's authorizer refused." },
        "404": { "description": "No such thing, or none this caller may see." },
    });

    if let Some(response) = body.and_then(|entry| entry.response) {
        let schema = response(response_gen);
        if let Some(ok) = responses.get_mut("200").and_then(Value::as_object_mut) {
            ok.insert(
                "content".to_owned(),
                json!({ "application/json": { "schema": schema } }),
            );
        }
    }

    let mut op = json!({
        "operationId": id,
        "summary": route.summary,
        "tags": [route.domain],
        "parameters": parameters,
        "security": [{ scheme(route.surface): [] }],
        "x-tezgah-permission": format!("{:?}", route.action).to_lowercase(),
        "responses": responses,
    });

    if let Some(request) = body.and_then(|entry| entry.request) {
        let schema = request(request_gen);
        if let Some(fields) = op.as_object_mut() {
            fields.insert(
                "requestBody".to_owned(),
                json!({
                    "required": true,
                    "content": { "application/json": { "schema": schema } },
                }),
            );
        }
    }

    op
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

/// The whole document. Deterministic: `serde_json` orders object keys, the
/// route table is read in the order it is declared, and [`BODIES`] is read in
/// the order it is declared too.
pub fn document() -> Value {
    let mut paths: Map<String, Value> = Map::new();
    let (mut request_gen, mut response_gen) = schemas();

    for route in routes() {
        let entry = paths
            .entry(route.path.to_owned())
            .or_insert_with(|| Value::Object(Map::new()));

        if let Some(object) = entry.as_object_mut() {
            object.insert(
                verb(route.method).to_owned(),
                operation(&route, &mut request_gen, &mut response_gen),
            );
        }
    }

    // The same name can come out of both generators — every id an operation's
    // body and its view share, today — and it is only ever safe because an id
    // newtype's schema does not depend on contract; request definitions win
    // the clash, on trust that it is one of those and not, say, a Decimal.
    let mut components: Map<String, Value> = Map::new();
    for (name, schema) in request_gen.definitions() {
        components.insert(name.clone(), schema.clone());
    }
    for (name, schema) in response_gen.definitions() {
        components
            .entry(name.clone())
            .or_insert_with(|| schema.clone());
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
            "schemas": components,
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

    #[test]
    fn a_page_is_one_schema_no_matter_how_many_operations_return_one() {
        let document = document();
        let schemas = document
            .pointer("/components/schemas")
            .and_then(Value::as_object)
            .expect("the document to carry components.schemas");

        assert!(
            schemas.contains_key("Page"),
            "no shared Page schema, though BODIES uses page_of three times"
        );
        let copies = schemas
            .keys()
            .filter(|name| name.starts_with("Page"))
            .count();
        assert_eq!(copies, 1, "Page must not be duplicated per item type");
    }

    #[test]
    fn money_crosses_the_wire_as_a_string_not_a_number() {
        let document = document();
        let amount = document
            .pointer("/components/schemas/BalanceView/properties/amount")
            .expect("BalanceView.amount to carry a schema");

        assert_eq!(
            amount.get("type").and_then(Value::as_str),
            Some("string"),
            "a Decimal must serialise as a string, not a number: {amount}"
        );
    }
}
