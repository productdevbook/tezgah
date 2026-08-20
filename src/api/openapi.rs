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

use super::{
    Method, QuerySchema, Route, Surface, admin_catalogue, admin_order, admin_rest, agreement,
    credit, digital, order_basket, payout, routes, store, subscription, tax_identity,
};

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

/// One parameter per field of the type the route declared its query string
/// as.
///
/// `of` registers that type — and anything it names, like a status enum — in
/// `components/schemas` and hands back the `$ref` that points at it; the
/// properties are then read back out of the generator. A field's own schema is
/// carried across as it stands, so a `$ref` inside one already points where
/// the document keeps it.
///
/// What this does not do is decide how a value is spelled in a URL. An array
/// field says `type: array` and nothing about commas or repetition, because
/// the handler's `serde` derive is what answers that and the derive is not
/// readable from here. A caller reading this document learns which parameters
/// exist and what each means, which is what it said nothing about before.
fn query_parameters(of: QuerySchema, generator: &mut SchemaGenerator) -> Vec<Value> {
    let reference = of(generator);
    let Some(name) = reference
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|path| path.rsplit('/').next())
        .map(str::to_owned)
    else {
        return Vec::new();
    };

    let Some(schema) = generator.definitions().get(&name) else {
        return Vec::new();
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };

    properties
        .iter()
        .map(|(field, shape)| {
            json!({
                "name": field,
                "in": "query",
                "required": required.contains(&field.as_str()),
                "schema": shape,
            })
        })
        .collect()
}

/// One operation's body schemas, keyed by the [`operation_id`] it belongs to.
struct Body {
    operation_id: &'static str,
    request: Option<SchemaFn>,
    response: Option<SchemaFn>,
}

/// The payout domain was the pilot for tezgah#202: request and response
/// bodies both, `Page<T>` used three different ways. Next came the seven
/// domains `client/` actually reads — catalogue, order, inventory,
/// customer, promotion, subscription, store — each wired for exactly the
/// view type `client/src/api/views.ts` hand-transcribes, on the list and
/// single-fetch operations that return it.
///
/// The `order` domain is filled in next, past what `client/` reads: every
/// operation across `admin_order`, `agreement` and `store` tagged `"order"`
/// in [`routes`] gets a body here, except the handful that answer `()` —
/// dropping an action off an open change, say — which have nothing to
/// schema. `admin_order::OrderView`, `admin_order::ReturnView` and
/// `admin_order::RequestReturn` share a short name with a distinct,
/// narrower `store::` type of the same purpose — the storefront's own
/// `#[schemars(rename = "Store…")]` on those three is what keeps the two
/// apart in `components/schemas` rather than a number schemars would
/// otherwise pick by walk order; `no_schema_name_was_disambiguated_by_a_number`
/// in `tests/openapi.rs` is the standing check that the next collision does
/// not get to choose a name that way either.
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
    // --------------------------------------------------------- catalogue
    Body {
        operation_id: "getAdminProducts",
        request: None,
        response: Some(page_of::<admin_catalogue::ProductView>),
    },
    Body {
        operation_id: "getAdminProductsById",
        request: None,
        response: Some(schema_of::<admin_catalogue::ProductView>),
    },
    Body {
        operation_id: "patchAdminProductsById",
        request: Some(schema_of::<admin_catalogue::UpdateProduct>),
        response: Some(schema_of::<admin_catalogue::ProductView>),
    },
    Body {
        operation_id: "deleteAdminProductsById",
        request: None,
        response: None,
    },
    // ------------------------------------------------------------- order
    Body {
        operation_id: "getAdminOrders",
        request: None,
        response: Some(page_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminOrdersById",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    // --------------------------------------------------------- inventory
    Body {
        operation_id: "getAdminInventoryItems",
        request: None,
        response: Some(page_of::<admin_catalogue::InventoryItemView>),
    },
    Body {
        operation_id: "getAdminInventoryItemsById",
        request: None,
        response: Some(schema_of::<admin_catalogue::InventoryItemView>),
    },
    Body {
        operation_id: "deleteAdminInventoryItemsById",
        request: None,
        response: None,
    },
    // ---------------------------------------------------------- customer
    Body {
        operation_id: "getAdminCustomers",
        request: None,
        response: Some(page_of::<admin_rest::CustomerView>),
    },
    Body {
        operation_id: "getAdminCustomersById",
        request: None,
        response: Some(schema_of::<admin_rest::CustomerView>),
    },
    Body {
        operation_id: "patchAdminCustomersById",
        request: Some(schema_of::<admin_rest::UpdateCustomer>),
        response: Some(schema_of::<admin_rest::CustomerView>),
    },
    Body {
        operation_id: "deleteAdminCustomersById",
        request: None,
        response: None,
    },
    // --------------------------------------------------------- promotion
    Body {
        operation_id: "getAdminPromotions",
        request: None,
        response: Some(page_of::<admin_rest::PromotionView>),
    },
    Body {
        operation_id: "getAdminPromotionsById",
        request: None,
        response: Some(schema_of::<admin_rest::PromotionView>),
    },
    Body {
        operation_id: "patchAdminPromotionsById",
        request: Some(schema_of::<admin_rest::UpdatePromotion>),
        response: Some(schema_of::<admin_rest::PromotionView>),
    },
    Body {
        operation_id: "deleteAdminPromotionsById",
        request: None,
        response: None,
    },
    // ------------------------------------------------------- subscription
    Body {
        operation_id: "getAdminSubscriptions",
        request: None,
        response: Some(page_of::<subscription::SubscriptionView>),
    },
    Body {
        operation_id: "getAdminSubscriptionsById",
        request: None,
        response: Some(schema_of::<subscription::SubscriptionView>),
    },
    // ------------------------------------------------------------- store
    Body {
        operation_id: "getAdminRegions",
        request: None,
        response: Some(page_of::<admin_rest::RegionView>),
    },
    Body {
        operation_id: "getAdminRegionsById",
        request: None,
        response: Some(schema_of::<admin_rest::RegionView>),
    },
    Body {
        operation_id: "patchAdminRegionsById",
        request: Some(schema_of::<admin_rest::UpdateRegion>),
        response: Some(schema_of::<admin_rest::RegionView>),
    },
    Body {
        operation_id: "getAdminSalesChannels",
        request: None,
        response: Some(page_of::<admin_rest::SalesChannelView>),
    },
    Body {
        operation_id: "getAdminSalesChannelsById",
        request: None,
        response: Some(schema_of::<admin_rest::SalesChannelView>),
    },
    Body {
        operation_id: "patchAdminSalesChannelsById",
        request: Some(schema_of::<admin_rest::UpdateSalesChannel>),
        response: Some(schema_of::<admin_rest::SalesChannelView>),
    },
    Body {
        operation_id: "deleteAdminSalesChannelsById",
        request: None,
        response: None,
    },
    // ==================================================================== order
    // --------------------------------------------------------------------- orders
    Body {
        operation_id: "postAdminOrders",
        request: Some(schema_of::<admin_order::CreateOrder>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdComplete",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdCancel",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdArchive",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "patchAdminOrdersByIdShippingAddress",
        request: Some(schema_of::<admin_order::AddressIn>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "patchAdminOrdersByIdBillingAddress",
        request: Some(schema_of::<admin_order::AddressIn>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "patchAdminOrdersByIdEmail",
        request: Some(schema_of::<admin_order::UpdateEmail>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdLineItems",
        request: None,
        response: Some(schema_of::<Vec<admin_order::LineItemView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdItems",
        request: None,
        response: Some(schema_of::<Vec<admin_order::OrderItemView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdShippingMethods",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ShippingMethodView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdSummary",
        request: None,
        response: Some(schema_of::<admin_order::SummaryView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdTotals",
        request: None,
        response: Some(schema_of::<admin_order::TotalsView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdLedger",
        request: None,
        response: Some(schema_of::<admin_order::LedgerView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdTransactions",
        request: None,
        response: Some(schema_of::<Vec<admin_order::TransactionView>>),
    },
    Body {
        operation_id: "postAdminOrdersByIdTransactions",
        request: Some(schema_of::<admin_order::RecordTransaction>),
        response: Some(schema_of::<admin_order::TransactionView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdPaymentCollection",
        request: Some(schema_of::<admin_order::AttachPaymentCollection>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdChanges",
        request: None,
        response: Some(page_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdReturns",
        request: None,
        response: Some(page_of::<admin_order::ReturnView>),
    },
    // --------------------------------------------------------------- draft orders
    Body {
        operation_id: "getAdminDraftOrders",
        request: None,
        response: Some(page_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "postAdminDraftOrders",
        request: Some(schema_of::<admin_order::CreateOrder>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminDraftOrdersById",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "deleteAdminDraftOrdersById",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "postAdminDraftOrdersByIdConvertToOrder",
        request: Some(schema_of::<admin_order::ConvertDraft>),
        response: Some(schema_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminDraftOrdersByIdEdit",
        request: None,
        response: Some(schema_of::<admin_order::ChangeDetailView>),
    },
    Body {
        operation_id: "postAdminDraftOrdersByIdEdit",
        request: Some(schema_of::<admin_order::OpenEdit>),
        response: Some(schema_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "deleteAdminDraftOrdersByIdEdit",
        request: Some(schema_of::<admin_order::DeclineChange>),
        response: Some(schema_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "postAdminDraftOrdersByIdEditItems",
        request: Some(schema_of::<admin_order::AddItemAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminDraftOrdersByIdEditShippingMethods",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminDraftOrdersByIdEditConfirm",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    // ---------------------------------------------------------------- order edits
    Body {
        operation_id: "getAdminOrdersByIdOrderEdits",
        request: None,
        response: Some(page_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdOrderEdits",
        request: Some(schema_of::<admin_order::OpenEdit>),
        response: Some(schema_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "getAdminOrderEditsById",
        request: None,
        response: Some(schema_of::<admin_order::ChangeDetailView>),
    },
    Body {
        operation_id: "deleteAdminOrderEditsById",
        request: Some(schema_of::<admin_order::DeclineChange>),
        response: Some(schema_of::<admin_order::ChangeView>),
    },
    Body {
        operation_id: "postAdminOrderEditsByIdItems",
        request: Some(schema_of::<admin_order::AddItemAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminOrderEditsByIdShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminOrderEditsByIdConfirm",
        request: None,
        response: Some(schema_of::<admin_order::OrderView>),
    },
    // -------------------------------------------------------------- order changes
    Body {
        operation_id: "getAdminOrderChangesById",
        request: None,
        response: Some(schema_of::<admin_order::ChangeDetailView>),
    },
    // -------------------------------------------------------------------- returns
    Body {
        operation_id: "getAdminReturns",
        request: None,
        response: Some(page_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "postAdminReturns",
        request: Some(schema_of::<admin_order::RequestReturn>),
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "getAdminReturnsById",
        request: None,
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "getAdminReturnsByIdItems",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ReturnItemView>>),
    },
    Body {
        operation_id: "postAdminReturnsByIdReceive",
        request: Some(schema_of::<admin_order::ReceiveReturn>),
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdDismissItems",
        request: Some(schema_of::<admin_order::ReceiveReturn>),
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdCancel",
        request: None,
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdRequestItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdReceiveItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminReturnsByIdRequest",
        request: None,
        response: Some(schema_of::<admin_order::ReturnView>),
    },
    // ------------------------------------------------------------------ exchanges
    Body {
        operation_id: "getAdminExchanges",
        request: None,
        response: Some(page_of::<admin_order::ExchangeView>),
    },
    Body {
        operation_id: "postAdminExchanges",
        request: Some(schema_of::<admin_order::RequestExchange>),
        response: Some(schema_of::<admin_order::ExchangeView>),
    },
    Body {
        operation_id: "getAdminExchangesById",
        request: None,
        response: Some(schema_of::<admin_order::ExchangeView>),
    },
    Body {
        operation_id: "getAdminExchangesByIdItems",
        request: None,
        response: Some(schema_of::<admin_order::ChangeDetailView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdCancel",
        request: None,
        response: Some(schema_of::<admin_order::ExchangeView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdInboundItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdInboundShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdOutboundItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdOutboundShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminExchangesByIdRequest",
        request: None,
        response: Some(schema_of::<admin_order::ExchangeView>),
    },
    // --------------------------------------------------------------------- claims
    Body {
        operation_id: "getAdminClaims",
        request: None,
        response: Some(page_of::<admin_order::ClaimView>),
    },
    Body {
        operation_id: "postAdminClaims",
        request: Some(schema_of::<admin_order::RequestClaim>),
        response: Some(schema_of::<admin_order::ClaimView>),
    },
    Body {
        operation_id: "getAdminClaimsById",
        request: None,
        response: Some(schema_of::<admin_order::ClaimView>),
    },
    Body {
        operation_id: "getAdminClaimsByIdLines",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ClaimItemView>>),
    },
    Body {
        operation_id: "getAdminClaimsByIdItems",
        request: None,
        response: Some(schema_of::<admin_order::ChangeDetailView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdCancel",
        request: None,
        response: Some(schema_of::<admin_order::ClaimView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdClaimItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdInboundItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdInboundShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdOutboundItems",
        request: Some(schema_of::<admin_order::LineQuantity>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdOutboundShippingMethod",
        request: Some(schema_of::<admin_order::AddShippingAction>),
        response: Some(schema_of::<admin_order::ChangeActionView>),
    },
    Body {
        operation_id: "postAdminClaimsByIdRequest",
        request: None,
        response: Some(schema_of::<admin_order::ClaimView>),
    },
    // ------------------------------------------------------------- return reasons
    Body {
        operation_id: "getAdminReturnReasons",
        request: None,
        response: Some(page_of::<admin_order::ReasonView>),
    },
    Body {
        operation_id: "postAdminReturnReasons",
        request: Some(schema_of::<admin_order::NewReason>),
        response: Some(schema_of::<admin_order::ReasonView>),
    },
    Body {
        operation_id: "getAdminReturnReasonsByIdTranslations",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ReturnReasonTranslationView>>),
    },
    Body {
        operation_id: "postAdminReturnReasonsByIdTranslations",
        request: Some(schema_of::<admin_order::PutReturnReasonTranslation>),
        response: Some(schema_of::<admin_order::ReturnReasonTranslationView>),
    },
    Body {
        operation_id: "getAdminReturnReasonsByIdTranslationsByLocale",
        request: None,
        response: Some(schema_of::<admin_order::LocalisedReturnReasonView>),
    },
    // ----------------------------------------------------------------- agreements
    Body {
        operation_id: "postAdminAgreements",
        request: Some(schema_of::<agreement::PublishAgreement>),
        response: Some(schema_of::<agreement::AgreementVersionView>),
    },
    Body {
        operation_id: "getAdminAgreements",
        request: None,
        response: Some(page_of::<agreement::AgreementVersionView>),
    },
    Body {
        operation_id: "getAdminAgreementsById",
        request: None,
        response: Some(schema_of::<agreement::AgreementVersionView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdAgreements",
        request: None,
        response: Some(schema_of::<Vec<agreement::OrderAgreementView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdAgreementsByKind",
        request: None,
        response: Some(schema_of::<agreement::AgreementVersionView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdWithdrawal",
        request: None,
        response: Some(schema_of::<Vec<agreement::WithdrawalView>>),
    },
    Body {
        operation_id: "postAdminReturnsByIdWithdrawal",
        request: None,
        response: Some(schema_of::<agreement::WithdrawalNoticeView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdInvoices",
        request: None,
        response: Some(schema_of::<Vec<agreement::InvoiceView>>),
    },
    Body {
        operation_id: "postAdminOrdersByIdInvoices",
        request: Some(schema_of::<agreement::RecordInvoice>),
        response: Some(schema_of::<agreement::InvoiceView>),
    },
    Body {
        operation_id: "postAdminOrdersByIdInvoicesByInvoiceIdCreditNote",
        request: Some(schema_of::<agreement::RecordInvoice>),
        response: Some(schema_of::<agreement::InvoiceView>),
    },
    Body {
        operation_id: "patchAdminInvoicesById",
        request: Some(schema_of::<agreement::SetInvoiceStatus>),
        response: Some(schema_of::<agreement::InvoiceView>),
    },
    Body {
        operation_id: "postStoreOrdersByIdAgreements",
        request: Some(schema_of::<agreement::AcceptAgreement>),
        response: Some(schema_of::<agreement::OrderAgreementView>),
    },
    Body {
        operation_id: "getStoreOrdersByIdAgreementsByKind",
        request: None,
        response: Some(schema_of::<agreement::AgreementVersionView>),
    },
    // ---------------------------------------------------------- storefront orders
    Body {
        operation_id: "getStoreOrders",
        request: None,
        response: Some(page_of::<store::OrderView>),
    },
    Body {
        operation_id: "getStoreOrdersById",
        request: None,
        response: Some(schema_of::<store::OrderView>),
    },
    Body {
        operation_id: "postStoreOrdersByIdTransferRequest",
        request: Some(schema_of::<store::RequestTransfer>),
        response: Some(schema_of::<store::RequestedTransferView>),
    },
    Body {
        operation_id: "postStoreOrdersByIdTransferAccept",
        request: Some(schema_of::<store::ClaimTransfer>),
        response: Some(schema_of::<store::OrderView>),
    },
    Body {
        operation_id: "postStoreOrdersByIdTransferDecline",
        request: Some(schema_of::<store::ClaimTransfer>),
        response: Some(schema_of::<store::TransferView>),
    },
    Body {
        operation_id: "postStoreOrdersByIdTransferCancel",
        request: None,
        response: Some(schema_of::<store::TransferView>),
    },
    Body {
        operation_id: "postStoreReturns",
        request: Some(schema_of::<store::RequestReturn>),
        response: Some(schema_of::<store::ReturnView>),
    },
    Body {
        operation_id: "getStoreReturnReasons",
        request: None,
        response: Some(page_of::<store::ReturnReasonView>),
    },
    Body {
        operation_id: "getStoreReturnReasonsById",
        request: None,
        response: Some(schema_of::<store::ReturnReasonView>),
    },
    // ============================================== the server binds (write)
    Body {
        operation_id: "postAdminCurrencies",
        request: Some(schema_of::<admin_rest::CreateCurrency>),
        response: Some(schema_of::<admin_rest::CurrencyView>),
    },
    Body {
        operation_id: "postAdminRegions",
        request: Some(schema_of::<admin_rest::CreateRegion>),
        response: Some(schema_of::<admin_rest::RegionView>),
    },
    Body {
        operation_id: "postAdminSalesChannels",
        request: Some(schema_of::<admin_rest::CreateSalesChannel>),
        response: Some(schema_of::<admin_rest::SalesChannelView>),
    },
    Body {
        operation_id: "postAdminPublishableApiKeys",
        request: Some(schema_of::<admin_rest::CreatePublishableKey>),
        response: Some(schema_of::<admin_rest::IssuedKeyView>),
    },
    Body {
        operation_id: "getAdminStockLocations",
        request: None,
        response: Some(page_of::<admin_catalogue::StockLocationView>),
    },
    Body {
        operation_id: "postAdminStockLocations",
        request: Some(schema_of::<admin_catalogue::CreateStockLocation>),
        response: Some(schema_of::<admin_catalogue::StockLocationView>),
    },
    Body {
        operation_id: "patchAdminStockLocationsById",
        request: Some(schema_of::<admin_catalogue::RenameStockLocation>),
        response: Some(schema_of::<admin_catalogue::StockLocationView>),
    },
    Body {
        operation_id: "deleteAdminStockLocationsById",
        request: None,
        response: None,
    },
    Body {
        operation_id: "postAdminProducts",
        request: Some(schema_of::<admin_catalogue::CreateProduct>),
        response: Some(schema_of::<admin_catalogue::ProductView>),
    },
    Body {
        operation_id: "postAdminProductsByIdVariants",
        request: Some(schema_of::<admin_catalogue::CreateVariant>),
        response: Some(schema_of::<admin_catalogue::VariantView>),
    },
    Body {
        operation_id: "postAdminPriceSets",
        request: None,
        response: Some(schema_of::<admin_catalogue::PriceSetView>),
    },
    Body {
        operation_id: "postAdminProductVariantsByIdPriceSet",
        request: Some(schema_of::<admin_catalogue::LinkPriceSet>),
        response: None,
    },
    Body {
        operation_id: "postAdminPrices",
        request: Some(schema_of::<admin_catalogue::AddPrice>),
        response: Some(schema_of::<admin_catalogue::PriceView>),
    },
    Body {
        operation_id: "postAdminInventoryItems",
        request: Some(schema_of::<admin_catalogue::CreateInventoryItem>),
        response: Some(schema_of::<admin_catalogue::InventoryItemView>),
    },
    Body {
        operation_id: "getAdminInventoryItemsByIdLocationLevels",
        request: None,
        response: Some(page_of::<admin_catalogue::InventoryLevelView>),
    },
    Body {
        operation_id: "getAdminInventoryItemsByIdLocationLevelsByLocationId",
        request: None,
        response: Some(schema_of::<admin_catalogue::InventoryLevelView>),
    },
    Body {
        operation_id: "postAdminInventoryItemsByIdLocationLevels",
        request: Some(schema_of::<admin_catalogue::SetStock>),
        response: Some(schema_of::<admin_catalogue::InventoryLevelView>),
    },
    // The three that take rows together. Left undocumented, each was a shape
    // a client transcribed from the Rust by hand and then had no way to
    // notice moving.
    Body {
        operation_id: "postAdminProductsBatch",
        request: Some(schema_of::<admin_catalogue::ImportProductsBody>),
        response: Some(schema_of::<admin_catalogue::ImportResultView>),
    },
    Body {
        operation_id: "postAdminPricesBatch",
        request: Some(schema_of::<admin_catalogue::UpdatePricesBody>),
        response: Some(schema_of::<admin_catalogue::BatchResultView>),
    },
    Body {
        operation_id: "postAdminInventoryItemsBatch",
        request: Some(schema_of::<admin_catalogue::SetStockLevelsBody>),
        response: Some(schema_of::<admin_catalogue::BatchResultView>),
    },
    // ----------------------------------------------------- order_basket
    Body {
        operation_id: "getAdminOrderBasketsById",
        request: None,
        response: Some(schema_of::<order_basket::BasketView>),
    },
    Body {
        operation_id: "getAdminOrderBasketsByIdOrders",
        request: None,
        response: Some(page_of::<admin_order::OrderView>),
    },
    Body {
        operation_id: "getAdminOrderBasketsByIdCarts",
        request: None,
        response: Some(page_of::<store::CartView>),
    },
    // ------------------------------------------------------------ workflow
    Body {
        operation_id: "getAdminWorkflowsExecutions",
        request: None,
        response: Some(page_of::<admin_rest::WorkflowRunSummaryView>),
    },
    Body {
        operation_id: "getAdminWorkflowsExecutionsById",
        request: None,
        response: Some(schema_of::<admin_rest::WorkflowRunView>),
    },
    Body {
        operation_id: "getAdminWorkflowsExecutionsByIdSteps",
        request: None,
        response: Some(schema_of::<Vec<admin_rest::WorkflowStepView>>),
    },
    Body {
        operation_id: "getAdminWorkflowDeadLetters",
        request: None,
        response: Some(page_of::<admin_rest::WorkflowDeadLetterView>),
    },
    // ----------------------------------------------------------- fulfilment
    Body {
        operation_id: "getAdminOrdersByIdFulfillments",
        request: None,
        response: Some(page_of::<admin_order::FulfillmentView>),
    },
    Body {
        operation_id: "getAdminOrdersByIdShippingOptions",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ShippingOptionView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdReturnsShippingOptions",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ShippingOptionView>>),
    },
    Body {
        operation_id: "getAdminOrdersByIdFulfillmentsByFulfillmentId",
        request: None,
        response: Some(schema_of::<admin_order::FulfillmentDetailView>),
    },
    Body {
        operation_id: "getAdminFulfillmentSets",
        request: None,
        response: Some(page_of::<admin_order::FulfillmentSetView>),
    },
    Body {
        operation_id: "getAdminFulfillmentSetsByIdServiceZones",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ServiceZoneView>>),
    },
    Body {
        operation_id: "getAdminFulfillmentProviders",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ProviderView>>),
    },
    Body {
        operation_id: "getAdminShippingOptions",
        request: None,
        response: Some(page_of::<admin_order::ShippingOptionView>),
    },
    Body {
        operation_id: "getAdminShippingOptionsById",
        request: None,
        response: Some(schema_of::<admin_order::ShippingOptionView>),
    },
    Body {
        operation_id: "getAdminShippingOptionsByIdTranslations",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ShippingOptionTranslationView>>),
    },
    Body {
        operation_id: "getAdminShippingOptionsByIdTranslationsByLocale",
        request: None,
        response: Some(schema_of::<admin_order::LocalisedShippingOptionView>),
    },
    Body {
        operation_id: "getAdminShippingProfiles",
        request: None,
        response: Some(page_of::<admin_order::ShippingProfileView>),
    },
    Body {
        operation_id: "getAdminShippingProfilesById",
        request: None,
        response: Some(schema_of::<admin_order::ShippingProfileView>),
    },
    Body {
        operation_id: "getAdminShippingOptionTypes",
        request: None,
        response: Some(page_of::<admin_order::ShippingOptionTypeView>),
    },
    Body {
        operation_id: "getStoreShippingOptions",
        request: None,
        response: Some(schema_of::<Vec<store::ShippingOptionView>>),
    },
    // ------------------------------------------------------------------ tax
    Body {
        operation_id: "getAdminTaxRegions",
        request: None,
        response: Some(page_of::<admin_rest::TaxRegionView>),
    },
    Body {
        operation_id: "getAdminTaxRegionsById",
        request: None,
        response: Some(schema_of::<admin_rest::TaxRegionView>),
    },
    Body {
        operation_id: "getAdminTaxRates",
        request: None,
        response: Some(page_of::<admin_rest::TaxRateView>),
    },
    Body {
        operation_id: "getAdminTaxRatesById",
        request: None,
        response: Some(schema_of::<admin_rest::TaxRateView>),
    },
    Body {
        operation_id: "getAdminTaxRatesByIdRules",
        request: None,
        response: Some(schema_of::<Vec<admin_rest::TaxRateRuleView>>),
    },
    Body {
        operation_id: "getAdminTaxRegistrations",
        request: None,
        response: Some(schema_of::<Vec<tax_identity::RegistrationView>>),
    },
    Body {
        operation_id: "getAdminCustomersByIdTaxIds",
        request: None,
        response: Some(schema_of::<Vec<tax_identity::TaxIdView>>),
    },
    Body {
        operation_id: "getAdminCustomersByIdTaxExemptions",
        request: None,
        response: Some(schema_of::<Vec<tax_identity::ExemptionView>>),
    },
    // -------------------------------------------------------------- pricing
    Body {
        operation_id: "getAdminPriceSetsById",
        request: None,
        response: Some(schema_of::<admin_catalogue::PriceSetView>),
    },
    Body {
        operation_id: "getAdminPriceSetsByIdPrices",
        request: None,
        response: Some(page_of::<admin_catalogue::PriceView>),
    },
    Body {
        operation_id: "getAdminProductVariantsByIdBundleComponents",
        request: None,
        response: Some(schema_of::<Vec<admin_catalogue::BundleComponentView>>),
    },
    Body {
        operation_id: "getAdminProductVariantsByIdBundlePrice",
        request: None,
        response: Some(schema_of::<admin_catalogue::BundlePriceView>),
    },
    Body {
        operation_id: "getAdminPricesByIdRules",
        request: None,
        response: Some(schema_of::<Vec<admin_catalogue::PriceRuleView>>),
    },
    Body {
        operation_id: "getAdminPriceLists",
        request: None,
        response: Some(page_of::<admin_catalogue::PriceListView>),
    },
    Body {
        operation_id: "getAdminPriceListsById",
        request: None,
        response: Some(schema_of::<admin_catalogue::PriceListView>),
    },
    Body {
        operation_id: "getAdminPricePreferences",
        request: None,
        response: Some(schema_of::<Option<admin_catalogue::PricePreferenceView>>),
    },
    // -------------------------------------------------------------- payment
    Body {
        operation_id: "getAdminPayments",
        request: None,
        response: Some(page_of::<admin_order::PaymentView>),
    },
    Body {
        operation_id: "getAdminPaymentsById",
        request: None,
        response: Some(schema_of::<admin_order::PaymentView>),
    },
    Body {
        operation_id: "getAdminPaymentsPaymentProviders",
        request: None,
        response: Some(schema_of::<Vec<admin_order::ProviderView>>),
    },
    Body {
        operation_id: "getAdminPaymentCollectionsById",
        request: None,
        response: Some(schema_of::<admin_order::CollectionView>),
    },
    Body {
        operation_id: "getAdminPaymentCollectionsByIdPaymentSessions",
        request: None,
        response: Some(page_of::<admin_order::SessionView>),
    },
    Body {
        operation_id: "getAdminRefundReasons",
        request: None,
        response: Some(page_of::<admin_order::ReasonView>),
    },
    Body {
        operation_id: "getStorePaymentProviders",
        request: None,
        response: Some(schema_of::<Vec<store::PaymentProviderView>>),
    },
    // -------------------------------------------------------------- credit
    Body {
        operation_id: "getAdminGiftCards",
        request: None,
        response: Some(page_of::<credit::GiftCardView>),
    },
    Body {
        operation_id: "getAdminGiftCardsById",
        request: None,
        response: Some(schema_of::<credit::GiftCardView>),
    },
    Body {
        operation_id: "getAdminGiftCardsByIdTransactions",
        request: None,
        response: Some(page_of::<credit::CreditMovementView>),
    },
    Body {
        operation_id: "getAdminCustomersByIdStoreCredit",
        request: None,
        response: Some(schema_of::<credit::StoreCreditView>),
    },
    Body {
        operation_id: "getAdminStoreCreditsByIdTransactions",
        request: None,
        response: Some(page_of::<credit::CreditMovementView>),
    },
    Body {
        operation_id: "getStoreCartsByIdCredits",
        request: None,
        response: Some(schema_of::<Vec<credit::CartCreditView>>),
    },
    Body {
        operation_id: "getStoreCustomersMeStoreCredit",
        request: None,
        response: Some(schema_of::<credit::StoreCreditView>),
    },
    // ------------------------------------------------------------- digital
    Body {
        operation_id: "getAdminOrdersByIdEntitlements",
        request: None,
        response: Some(schema_of::<Vec<digital::EntitlementView>>),
    },
    Body {
        operation_id: "postAdminOrdersByIdEntitlementsRevoke",
        request: Some(schema_of::<digital::RevokeEntitlements>),
        response: Some(schema_of::<Vec<digital::EntitlementView>>),
    },
    Body {
        operation_id: "getAdminVariantsByIdDigitalContent",
        request: None,
        response: Some(schema_of::<Vec<digital::ContentView>>),
    },
    Body {
        operation_id: "postAdminVariantsByIdDigitalContent",
        request: Some(schema_of::<digital::PutContent>),
        response: Some(schema_of::<digital::ContentView>),
    },
    Body {
        operation_id: "getStoreEntitlements",
        request: None,
        response: Some(page_of::<digital::EntitlementView>),
    },
    Body {
        operation_id: "postStoreEntitlementsByIdToken",
        request: None,
        response: Some(schema_of::<digital::TokenView>),
    },
    Body {
        operation_id: "postStoreDownloads",
        request: Some(schema_of::<digital::Redeem>),
        response: Some(schema_of::<digital::DownloadView>),
    },
    // --------------------------------------------------------------- cart
    Body {
        operation_id: "getStoreCartsByIdLineItems",
        request: None,
        response: Some(schema_of::<Vec<store::LineItemView>>),
    },
    Body {
        operation_id: "getAdminCarts",
        request: None,
        response: Some(page_of::<store::CartView>),
    },
];

fn operation(
    route: &Route,
    request_gen: &mut SchemaGenerator,
    response_gen: &mut SchemaGenerator,
) -> Value {
    let id = operation_id(route);
    let body = BODIES.iter().find(|entry| entry.operation_id == id);

    let mut parameters: Vec<Value> = parameters(route.path)
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

    // Declared by the route itself, and described by the same generator the
    // request bodies use: what a handler deserialises is what a caller sends.
    if let Some(of) = route.query {
        parameters.extend(query_parameters(of, request_gen));
    }

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
    // the clash. That is enforced, not trusted: tests/openapi.rs's
    // colliding_names_agree_across_generators calls schema_collisions() below
    // and fails the build the day a colliding name's two schemas disagree.
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

/// Every name [`BODIES`] makes both generators define, paired with what each
/// one says it is. `document()` trusts a colliding name to mean the same
/// thing on a request as on a response; this is what a caller checks that
/// trust against, rather than reading it off `document()`'s own merged
/// output, which has already picked a side and cannot say what the discarded
/// one was.
pub fn schema_collisions() -> Vec<(String, Value, Value)> {
    let (mut request_gen, mut response_gen) = schemas();
    for body in BODIES {
        if let Some(request) = body.request {
            request(&mut request_gen);
        }
        if let Some(response) = body.response {
            response(&mut response_gen);
        }
    }

    let responded: std::collections::BTreeMap<String, Value> = response_gen
        .definitions()
        .iter()
        .map(|(name, schema)| (name.clone(), schema.clone()))
        .collect();

    request_gen
        .definitions()
        .iter()
        .filter_map(|(name, requested)| {
            responded
                .get(name)
                .map(|answered| (name.clone(), requested.clone(), answered.clone()))
        })
        .collect()
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
            "no shared Page schema, though BODIES reaches for page_of"
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

        // Every `rust_decimal::Decimal` field BODIES currently reaches,
        // response side: payout's own money, catalogue's dimensions, the
        // order domain's — `MoneyView` once for every view built through
        // `amount_view`/`From<Money>`, plus the two invoices carry a total
        // outside that helper — pricing's own `PriceView.amount` and a
        // bundle's own priced total and its components' shares, and a
        // payment collection's own four running totals, which `CollectionView`
        // carries as raw `Decimal` rather than through `amount_view` because
        // all four already share the collection's one fixed currency, and a
        // gift card's, a store credit's and a cart credit's own balances and
        // movements, all riding the same `for_serialize` generator and
        // answering the same way even though not all of them are money.
        for pointer in [
            "/components/schemas/BalanceView/properties/amount",
            "/components/schemas/PayoutView/properties/amount",
            "/components/schemas/PayoutLineView/properties/amount",
            "/components/schemas/CommissionRuleView/properties/value",
            "/components/schemas/ProductView/properties/weight",
            "/components/schemas/ProductView/properties/length",
            "/components/schemas/ProductView/properties/height",
            "/components/schemas/ProductView/properties/width",
            "/components/schemas/MoneyView/properties/amount",
            "/components/schemas/InvoiceView/properties/total_amount",
            "/components/schemas/GiftCardView/properties/initial_balance",
            "/components/schemas/GiftCardView/properties/balance",
            "/components/schemas/CreditMovementView/properties/amount",
            "/components/schemas/StoreCreditView/properties/balance",
            "/components/schemas/CartCreditView/properties/amount",
            "/components/schemas/PriceView/properties/amount",
            "/components/schemas/BundlePriceView/properties/total",
            "/components/schemas/BundlePriceComponentView/properties/unit_price",
            "/components/schemas/BundlePriceComponentView/properties/allocated_total",
            "/components/schemas/CollectionView/properties/amount",
            "/components/schemas/CollectionView/properties/authorized_amount",
            "/components/schemas/CollectionView/properties/captured_amount",
            "/components/schemas/CollectionView/properties/refunded_amount",
        ] {
            let schema = document
                .pointer(pointer)
                .unwrap_or_else(|| panic!("{pointer} to carry a schema"));
            let types: Vec<&str> = match schema.get("type") {
                Some(Value::String(one)) => vec![one.as_str()],
                Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
                other => panic!("{pointer} has no usable \"type\": {other:?}"),
            };
            assert!(
                types.contains(&"string"),
                "{pointer} must serialise as a string: {schema}"
            );
            assert!(
                !types.contains(&"number"),
                "{pointer} answers a number on the response contract: {schema}"
            );
        }

        // Every Decimal BODIES reaches on the request side accepts both,
        // because that is what serde-with-str actually parses: payout's own
        // commission rate, `MoneyIn.amount` behind every order-domain write
        // that takes an amount, the invoice's own total, `AddPrice.amount`
        // behind `POST /admin/prices`, `UpdateProduct`'s four dimensions
        // behind `PATCH /admin/products/{id}`, and the two the batch routes
        // reach — a price change's amount and an imported row's.
        for pointer in [
            "/components/schemas/SetCommissionRule/properties/value",
            "/components/schemas/MoneyIn/properties/amount",
            "/components/schemas/RecordInvoice/properties/total",
            "/components/schemas/AddPrice/properties/amount",
            "/components/schemas/UpdateProduct/properties/weight",
            "/components/schemas/UpdateProduct/properties/length",
            "/components/schemas/UpdateProduct/properties/height",
            "/components/schemas/UpdateProduct/properties/width",
            "/components/schemas/PriceChangeRow/properties/amount",
            "/components/schemas/ImportRow/properties/price_amount",
        ] {
            let value = document
                .pointer(pointer)
                .unwrap_or_else(|| panic!("{pointer} to carry a schema"));
            let types: Vec<&str> = value
                .get("type")
                .and_then(Value::as_array)
                .map(|many| many.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            assert!(
                types.contains(&"string") && types.contains(&"number"),
                "a request Decimal must accept both string and number: {value}"
            );
        }
    }
}
