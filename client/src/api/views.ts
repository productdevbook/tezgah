import { z } from "zod"

import type { components } from "./schema"

/**
 * What the admin surface answers with, as schemas that are checked at runtime.
 *
 * **Transcribed from the Rust by hand**, and that is not the intended state:
 * `tests/snapshots/openapi.json` declares 483 operations and zero schemas, so
 * nothing can generate these (productdevbook/tezgah#202).
 *
 * They are zod rather than plain types for exactly that reason. A hand-written
 * type drifts in silence — a renamed field becomes `undefined` in a cell and
 * nobody learns anything. Parsed at the boundary, the same drift names the
 * field it happened to, on the first response. The cost is a parse per
 * response; the thing bought is that this file cannot quietly become fiction.
 *
 * Each schema names the Rust struct it mirrors, so checking one is a grep.
 */

/** `src/page.rs` — `Page<T>`. `next` is the cursor; absent on the last page. */
export const page = <T extends z.ZodTypeAny>(item: T) =>
  z.object({ items: z.array(item), next: z.string().nullable() })

export type Page<T> = { items: T[]; next: string | null }

/**
 * A uuid as the crate writes it. Not `z.uuid()`: these are uuidv7 and the
 * version nibble is the point — a v4 here would mean something upstream
 * stopped using the id type.
 */
const id = z.string()

/** `chrono::DateTime<Utc>`, serde's RFC 3339. */
const timestamp = z.string()

/**
 * `rust_decimal` under `serde-with-str`: a string on the wire, never a number.
 * Reading it as a number is how precision is lost, and this is money.
 */
const decimal = z.string()

/** `src/catalogue.rs` — `ProductStatus`, serialized lowercase. */
export const productStatus = z.enum([
  "draft",
  "proposed",
  "published",
  "archived",
  "rejected",
])
export type ProductStatus = z.infer<typeof productStatus>

/** `src/api/admin_catalogue.rs` — `ProductView`. */
export const product = z.object({
  id,
  handle: z.string(),
  title: z.string(),
  subtitle: z.string().nullable(),
  description: z.string().nullable(),
  status: productStatus,
  rejected_reason: z.string().nullable(),
  thumbnail_url: z.string().nullable(),
  is_discountable: z.boolean(),
  product_type_id: id.nullable(),
  product_collection_id: id.nullable(),
  weight: decimal.nullable(),
  length: decimal.nullable(),
  height: decimal.nullable(),
  width: decimal.nullable(),
  material: z.string().nullable(),
  hs_code: z.string().nullable(),
  origin_country: z.string().nullable(),
  external_id: z.string().nullable(),
  metadata: z.unknown(),
  created_at: timestamp,
  updated_at: timestamp,
})
export type Product = z.infer<typeof product>

/** `src/api/admin_order.rs` — `OrderView`. */
export const order = z.object({
  id,
  display_id: z.number().nullable(),
  email: z.string().nullable(),
  currency_code: z.string(),
  version: z.number(),
  status: z.string(),
  payment_status: z.string(),
  fulfillment_status: z.string(),
  is_draft: z.boolean(),
  payment_collection_id: id.nullable(),
  basket_id: id.nullable(),
  completed_at: timestamp.nullable(),
  canceled_at: timestamp.nullable(),
  created_at: timestamp,
})
export type Order = z.infer<typeof order>

/** `src/api/admin_catalogue.rs` — `InventoryItemView`. */
export const inventoryItem = z.object({
  id,
  sku: z.string().nullable(),
  title: z.string().nullable(),
  requires_shipping: z.boolean(),
  created_at: timestamp,
})
export type InventoryItem = z.infer<typeof inventoryItem>

/** `src/api/admin_rest.rs` — `CustomerView`. */
export const customer = z.object({
  id,
  email: z.string().nullable(),
  first_name: z.string().nullable(),
  last_name: z.string().nullable(),
  phone: z.string().nullable(),
  company_name: z.string().nullable(),
  has_account: z.boolean(),
  anonymised: z.boolean(),
  metadata: z.unknown().nullable(),
  created_at: timestamp,
  updated_at: timestamp,
})
export type Customer = z.infer<typeof customer>

/** `src/api/admin_rest.rs` — `PromotionView`. */
export const promotion = z.object({
  id,
  campaign_id: id.nullable(),
  code: z.string(),
  kind: z.string(),
  status: z.string(),
  is_automatic: z.boolean(),
  usage_limit: z.number().nullable(),
  used: z.number(),
  customer_usage_limit: z.number().nullable(),
  metadata: z.unknown().nullable(),
  created_at: timestamp,
})
export type Promotion = z.infer<typeof promotion>

/** `src/api/subscription.rs` — `SubscriptionView`. */
export const subscription = z.object({
  id,
  customer_id: id,
  selling_plan_id: id,
  currency_code: z.string(),
  status: z.string(),
  next_billing_at: timestamp,
  current_period_start: timestamp,
  current_period_end: timestamp,
  cycle: z.number(),
  cancel_at_period_end: z.boolean(),
  ended_at: timestamp.nullable(),
  dunning_attempts: z.number(),
})
export type Subscription = z.infer<typeof subscription>

/** `src/api/admin_rest.rs` — `RegionView`. */
export const region = z.object({
  id,
  name: z.string(),
  currency_code: z.string(),
  is_tax_inclusive: z.boolean(),
  has_automatic_taxes: z.boolean(),
  payment_providers: z.array(z.string()),
  created_at: timestamp,
})
export type Region = z.infer<typeof region>

/** `src/api/admin_rest.rs` — `SalesChannelView`. */
export const salesChannel = z.object({
  id,
  name: z.string(),
  description: z.string().nullable(),
  is_disabled: z.boolean(),
  created_at: timestamp,
})
export type SalesChannel = z.infer<typeof salesChannel>

/**
 * What stops the schemas above being wrong.
 *
 * Each line asserts that the type zod produces is *exactly* the type the
 * document declares — not merely assignable, so a field the crate adds is
 * caught as surely as one it renames. A mismatch is a build failure here
 * rather than a blank cell in a table, which is the whole reason these are
 * worth hand-writing while #202 is only part done.
 *
 * `schema.d.ts` is generated from `tests/snapshots/openapi.json`, and CI
 * refuses a stale one. So the chain runs all the way: change a view struct in
 * Rust, and either this file changes with it or the build stops.
 */
type Declared = components["schemas"]

/** Invariant equality, not assignability: extra fields fail too. */
type Exact<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2
    ? true
    : { mismatch: [A, B] }

type Assert<T extends true> = T

export type SchemasMatchTheDocument = [
  Assert<Exact<z.infer<typeof product>, Declared["ProductView"]>>,
  Assert<Exact<z.infer<typeof order>, Declared["OrderView"]>>,
  Assert<Exact<z.infer<typeof inventoryItem>, Declared["InventoryItemView"]>>,
  Assert<Exact<z.infer<typeof customer>, Declared["CustomerView"]>>,
  Assert<Exact<z.infer<typeof promotion>, Declared["PromotionView"]>>,
  Assert<Exact<z.infer<typeof subscription>, Declared["SubscriptionView"]>>,
  Assert<Exact<z.infer<typeof region>, Declared["RegionView"]>>,
  Assert<Exact<z.infer<typeof salesChannel>, Declared["SalesChannelView"]>>,
]
