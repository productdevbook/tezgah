import { z } from "zod"

import { GetAdminCustomersByIdResponse } from "@/api/generated/zod/customer/customer"
import { GetAdminInventoryItemsByIdResponse } from "@/api/generated/zod/inventory/inventory"
import { GetAdminOrdersByIdResponse } from "@/api/generated/zod/order/order"
import { GetAdminProductsByIdResponse } from "@/api/generated/zod/catalogue/catalogue"
import { GetAdminPromotionsByIdResponse } from "@/api/generated/zod/promotion/promotion"
import {
  GetAdminRegionsByIdResponse,
  GetAdminSalesChannelsByIdResponse,
} from "@/api/generated/zod/store/store"
import { GetAdminSubscriptionsByIdResponse } from "@/api/generated/zod/subscription/subscription"

/**
 * What the admin surface answers with, as schemas checked at runtime.
 *
 * `tests/snapshots/openapi.json` currently documents 22 of 483 operations'
 * bodies (productdevbook/tezgah#202 is the rest). Where it does, the schema
 * below is generated — `../generated/zod/**`, from `orval.config.ts`'s
 * `zod` project, re-exported here under the name every screen already used.
 * Where it does not, the schema is hand-written and says so; only a diff
 * against the Rust struct it names keeps that half honest.
 *
 * Either way they are zod, parsed at the boundary (`api/drift.ts`), for the
 * same reason: a hand-written type drifts in silence — a renamed field
 * becomes `undefined` in a cell and nobody learns anything. Parsed, the same
 * drift is its own error kind, `drifted`, and the screen says which field it
 * happened to.
 */

/** `src/page.rs` — `Page<T>`. `next` is the cursor; absent on the last page. */
export const page = <T extends z.ZodTypeAny>(item: T) =>
  z.object({ items: z.array(item), next: z.string().nullable() })

export type Page<T> = { items: T[]; next: string | null }

// — generated: GET .../{id} documents the item alone, so that response is
// the canonical schema for the type — the corresponding list is the same
// shape under `page()`, not a second declaration of it.

export const product = GetAdminProductsByIdResponse
export type Product = z.infer<typeof product>

export const productStatus = product.shape.status
export type ProductStatus = z.infer<typeof productStatus>

export const order = GetAdminOrdersByIdResponse
export type Order = z.infer<typeof order>

export const inventoryItem = GetAdminInventoryItemsByIdResponse
export type InventoryItem = z.infer<typeof inventoryItem>

export const customer = GetAdminCustomersByIdResponse
export type Customer = z.infer<typeof customer>

export const promotion = GetAdminPromotionsByIdResponse
export type Promotion = z.infer<typeof promotion>

export const subscription = GetAdminSubscriptionsByIdResponse
export type Subscription = z.infer<typeof subscription>

export const region = GetAdminRegionsByIdResponse
export type Region = z.infer<typeof region>

export const salesChannel = GetAdminSalesChannelsByIdResponse
export type SalesChannel = z.infer<typeof salesChannel>

// — hand-written: #202 does not document these yet, so each is transcribed
// straight from the Rust struct it names, same as before orval existed.

/**
 * `src/api/admin_catalogue.rs` — `CreateProduct`. Its full request has
 * eighteen fields; this is the subset the "New product" form sends, every
 * one of them present in the Rust struct with the type it declares there.
 */
export const createProduct = z.object({
  handle: z
    .string()
    .trim()
    .min(1, "a handle is needed")
    .regex(/^\S+$/, "a handle has no spaces in it"),
  title: z.string().trim().min(1, "a title is needed"),
  subtitle: z.string().trim().optional(),
  description: z.string().trim().optional(),
  status: productStatus.optional(),
})
export type CreateProduct = z.infer<typeof createProduct>

/** `src/api/admin_rest.rs` — `CreateCurrency`. */
export const createCurrency = z.object({
  code: z
    .string()
    .trim()
    .length(3, "a currency code is three letters")
    .regex(/^[A-Za-z]{3}$/, "a currency code is three letters"),
  numeric_code: z.string().trim().optional(),
  exponent: z
    .number()
    .int()
    .min(0)
    .max(4, "a currency's exponent is between 0 and 4"),
  symbol: z.string().trim().min(1, "a currency needs a symbol"),
  symbol_native: z.string().trim().min(1, "a currency needs a native symbol"),
  name: z.string().trim().min(1, "a currency needs a name"),
})
export type CreateCurrency = z.infer<typeof createCurrency>

/** `src/api/admin_rest.rs` — `CurrencyView`. Not documented — see above. */
export const currency = z.object({
  code: z.string(),
  symbol: z.string(),
  name: z.string(),
  exponent: z.number(),
})
export type Currency = z.infer<typeof currency>

/** `src/api/admin_rest.rs` — `CreateRegion`. */
export const createRegion = z.object({
  name: z.string().trim().min(1, "a region needs a name"),
  currency_code: z
    .string()
    .trim()
    .length(3, "a currency code is three letters")
    .regex(/^[A-Za-z]{3}$/, "a currency code is three letters"),
  is_tax_inclusive: z.boolean(),
  has_automatic_taxes: z.boolean(),
})
export type CreateRegion = z.infer<typeof createRegion>

/** `src/api/admin_rest.rs` — `CreateSalesChannel`. */
export const createSalesChannel = z.object({
  name: z.string().trim().min(1, "a sales channel needs a name"),
  description: z.string().trim().optional(),
  is_disabled: z.boolean(),
})
export type CreateSalesChannel = z.infer<typeof createSalesChannel>

/** `src/api/admin_rest.rs` — `CreatePublishableKey`. */
export const createPublishableKey = z.object({
  title: z.string().trim().min(1, "a publishable key needs a title"),
})
export type CreatePublishableKey = z.infer<typeof createPublishableKey>

/**
 * `src/api/admin_rest.rs` — `IssuedKeyView`: `PublishableKeyView` flattened
 * with the raw `token`, sent once. Not documented — see above.
 */
export const issuedKey = z.object({
  id: z.string(),
  title: z.string(),
  revoked_at: z.string().nullable(),
  last_used_at: z.string().nullable(),
  created_at: z.string(),
  token: z.string(),
})
export type IssuedKey = z.infer<typeof issuedKey>
