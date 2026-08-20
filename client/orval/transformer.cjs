// Orval 8.24.0 does `'propertyNames' in schema` while walking a subschema, and
// a bare JSON Schema boolean (`true` meaning "any value", `false` meaning
// "nothing") is not an object — the `in` operator throws on it. tezgah's own
// document uses that shorthand deliberately: `Page.items.items` and every
// `serde_json::Value` field (`ProductView.metadata`, ...) are declared as bare
// `true`, which is the 2020-12 way to say "no constraint" rather than a
// specific object shape.
//
// So: give orval an object in every position that would otherwise be a bare
// boolean. `true` becomes `{}` (still "anything"), `false` becomes
// `{ not: {} }` (still "nothing"). Both are semantically identical JSON
// Schema — this changes nothing about what a caller may send or receive.
//
// A second thing orval gets wrong, and the same treatment: a schema whose
// `type` is a list — money on a request is `["string", "number"]`, because
// rust_decimal deserialises from either — with a `default` beside it. Orval
// emits a zod union and gives *both* branches the same default constant, so
// the numeric branch ends up as `z.number().default("0")` and does not
// typecheck. Dropping the default from orval's copy costs nothing: the server
// applies it regardless of what a generated client says, and the document
// itself keeps it.
//
// This runs only on the copy orval parses in memory (`input.override.transformer`);
// `tests/snapshots/openapi.json` itself is never touched.

const SCHEMA_KEY = new Set([
  "additionalProperties",
  "additionalItems",
  "contains",
  "else",
  "if",
  "items",
  "not",
  "propertyNames",
  "then",
  "unevaluatedItems",
  "unevaluatedProperties",
  "schema",
])

const SCHEMA_MAP_KEY = new Set(["properties", "patternProperties"])

const SCHEMA_ARRAY_KEY = new Set(["allOf", "anyOf", "oneOf", "prefixItems"])

function fixBareBoolean(schema) {
  if (schema === true) return {}
  if (schema === false) return { not: {} }
  return fix(schema)
}

function fix(node) {
  if (Array.isArray(node)) return node.map(fix)
  if (node === null || typeof node !== "object") return node

  const out = {}
  // A union of types cannot carry one default that fits every branch.
  if (Array.isArray(node.type) && node.type.length > 1 && "default" in node) {
    const { default: _dropped, ...rest } = node
    node = rest
  }
  for (const [key, value] of Object.entries(node)) {
    if (SCHEMA_KEY.has(key)) {
      out[key] = fixBareBoolean(value)
    } else if (SCHEMA_MAP_KEY.has(key) && value && typeof value === "object") {
      const map = {}
      for (const [name, sub] of Object.entries(value)) map[name] = fixBareBoolean(sub)
      out[key] = map
    } else if (SCHEMA_ARRAY_KEY.has(key) && Array.isArray(value)) {
      out[key] = value.map(fixBareBoolean)
    } else {
      out[key] = fix(value)
    }
  }
  return out
}

module.exports = function transformer(inputSchema) {
  return fix(inputSchema)
}
