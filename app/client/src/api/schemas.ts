import { z } from "zod"

import {
  GetAdminCustomersByIdResponse,
  PatchAdminCustomersByIdBody,
} from "@/api/generated/zod/customer/customer"
import {
  GetAdminInventoryItemsByIdLocationLevelsByLocationIdResponse,
  GetAdminInventoryItemsByIdResponse,
  PostAdminStockLocationsResponse,
} from "@/api/generated/zod/inventory/inventory"
import { GetAdminOrdersByIdResponse } from "@/api/generated/zod/order/order"
import {
  GetAdminProductsByIdResponse,
  PatchAdminProductsByIdBody,
  PostAdminProductsByIdVariantsResponse,
} from "@/api/generated/zod/catalogue/catalogue"
import {
  GetAdminPromotionsByIdResponse,
  PatchAdminPromotionsByIdBody,
} from "@/api/generated/zod/promotion/promotion"
import {
  GetAdminRegionsByIdResponse,
  GetAdminSalesChannelsByIdResponse,
  PatchAdminRegionsByIdBody,
  PatchAdminSalesChannelsByIdBody,
} from "@/api/generated/zod/store/store"
import { GetAdminSubscriptionsByIdResponse } from "@/api/generated/zod/subscription/subscription"
import {
  GetAdminFulfillmentProvidersResponseItem,
  GetAdminShippingOptionsByIdResponse,
  GetAdminShippingProfilesByIdResponse,
} from "@/api/generated/zod/fulfilment/fulfilment"
import {
  GetAdminTaxRatesByIdResponse,
  GetAdminTaxRegionsByIdResponse,
  GetAdminTaxRegistrationsResponseItem,
} from "@/api/generated/zod/tax/tax"
import {
  GetAdminPriceListsByIdResponse,
  GetAdminPricePreferencesResponse,
  GetAdminPriceSetsByIdResponse,
} from "@/api/generated/zod/pricing/pricing"
import { GetAdminPaymentsByIdResponse } from "@/api/generated/zod/payment/payment"
import { GetAdminGiftCardsByIdResponse } from "@/api/generated/zod/credit/credit"
import {
  GetAdminOrdersByIdEntitlementsResponseItem,
  PostAdminOrdersByIdEntitlementsRevokeBody,
  GetAdminVariantsByIdDigitalContentResponseItem,
  PostAdminVariantsByIdDigitalContentBody,
} from "@/api/generated/zod/digital/digital"

/**
 * What the admin surface answers with, as schemas checked at runtime.
 *
 * `tests/snapshots/openapi.json` currently documents 22 of 486 operations'
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
  z.object({
    items: z.array(item),
    next: z.string().nullable(),
    // Absent unless the caller asked and the list can answer, so `.optional()`
    // as well as nullable. Zero is a real count and stays distinguishable from
    // "nobody asked" — a screen showing "0 of 0" for an uncounted list would
    // be inventing the second number.
    total: z.number().nullable().optional(),
  })

export type Page<T> = {
  items: T[]
  next: string | null
  total?: number | null
}

// — generated: GET .../{id} documents the item alone, so that response is
// the canonical schema for the type — the corresponding list is the same
// shape under `page()`, not a second declaration of it.

export const product = GetAdminProductsByIdResponse
export type Product = z.infer<typeof product>

export const productStatus = product.shape.status
export type ProductStatus = z.infer<typeof productStatus>

/**
 * `PATCH /admin/products/{id}`'s body — a field left out is left alone, and
 * `null` on a nullable one clears it. `edit.tsx` sends a subset of these
 * keys; the rest come through as `undefined`, which this schema already
 * treats as "unchanged".
 */
export const updateProduct = PatchAdminProductsByIdBody
export type UpdateProduct = z.infer<typeof updateProduct>

export const order = GetAdminOrdersByIdResponse
export type Order = z.infer<typeof order>

export const inventoryItem = GetAdminInventoryItemsByIdResponse
export type InventoryItem = z.infer<typeof inventoryItem>

/**
 * The three below are named after the route that returns one on its own,
 * which for two of them is a `POST`.
 *
 * A page's schema is generated as an intersection, so the item inside it is
 * not exported by itself; a variant and a stock location have no `GET` of
 * their own to be named after. Creating one answers with the same view the
 * page carries, so this is that view — generated, not transcribed.
 */
export const inventoryLevel =
  GetAdminInventoryItemsByIdLocationLevelsByLocationIdResponse
export type InventoryLevel = z.infer<typeof inventoryLevel>

export const variant = PostAdminProductsByIdVariantsResponse
export type Variant = z.infer<typeof variant>

export const stockLocation = PostAdminStockLocationsResponse
export type StockLocation = z.infer<typeof stockLocation>

export const customer = GetAdminCustomersByIdResponse
export type Customer = z.infer<typeof customer>

export const updateCustomer = PatchAdminCustomersByIdBody
export type UpdateCustomer = z.infer<typeof updateCustomer>

export const promotion = GetAdminPromotionsByIdResponse
export type Promotion = z.infer<typeof promotion>

export const updatePromotion = PatchAdminPromotionsByIdBody
export type UpdatePromotion = z.infer<typeof updatePromotion>

export const subscription = GetAdminSubscriptionsByIdResponse
export type Subscription = z.infer<typeof subscription>

/**
 * `src/api/subscription.rs` — `ContractView`: `SubscriptionView` flattened
 * with `lines`. `GET /admin/subscriptions/{id}` returns this, not
 * `SubscriptionView` alone, but `src/api/openapi.rs` names the latter for
 * that operation — a documentation bug, not a client one — so `lines` is
 * added by hand rather than trusted to the generated schema above.
 */
export const subscriptionLine = z.object({
  variant_id: z.string(),
  title: z.string().nullable(),
  quantity: z.number().int(),
  unit_price: z.string(),
  currency_code: z.string(),
})
export type SubscriptionLine = z.infer<typeof subscriptionLine>

export const subscriptionContract = subscription.extend({
  lines: z.array(subscriptionLine),
})
export type SubscriptionContract = z.infer<typeof subscriptionContract>

export const region = GetAdminRegionsByIdResponse
export type Region = z.infer<typeof region>

export const updateRegion = PatchAdminRegionsByIdBody
export type UpdateRegion = z.infer<typeof updateRegion>

export const salesChannel = GetAdminSalesChannelsByIdResponse
export type SalesChannel = z.infer<typeof salesChannel>

export const updateSalesChannel = PatchAdminSalesChannelsByIdBody
export type UpdateSalesChannel = z.infer<typeof updateSalesChannel>

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
 * `src/api/admin_rest.rs` — `PublishableKeyView`. What `GET
 * /admin/publishable-api-keys` lists — no `token`, which only the moment a
 * key is minted ever carries. Not documented — see above.
 */
export const publishableKey = z.object({
  id: z.string(),
  title: z.string(),
  revoked_at: z.string().nullable(),
  last_used_at: z.string().nullable(),
  created_at: z.string(),
})
export type PublishableKey = z.infer<typeof publishableKey>

/**
 * `src/api/admin_rest.rs` — `IssuedKeyView`: `PublishableKeyView` flattened
 * with the raw `token`, sent once. Not documented — see above.
 */
export const issuedKey = publishableKey.extend({ token: z.string() })
export type IssuedKey = z.infer<typeof issuedKey>

/** `src/api/payout.rs` — `CommissionRuleView`. Not documented — see above. */
export const commissionRule = z.object({
  id: z.string(),
  category_id: z.string().nullable(),
  kind: z.enum(["fixed", "percentage"]),
  value: z.string(),
  currency_code: z.string().nullable(),
})
export type CommissionRule = z.infer<typeof commissionRule>

/** `src/api/payout.rs` — `PayoutLineView`. Not documented — see above. */
export const payoutLine = z.object({
  id: z.string(),
  order_id: z.string().nullable(),
  payout_id: z.string().nullable(),
  amount: z.string(),
  currency_code: z.string(),
  reference: z.string(),
  reference_id: z.string().nullable(),
})
export type PayoutLine = z.infer<typeof payoutLine>

/** `src/api/payout.rs` — `PayoutView`. Not documented — see above. */
export const payout = z.object({
  id: z.string(),
  amount: z.string(),
  currency_code: z.string(),
  reference: z.string(),
  reference_id: z.string(),
})
export type Payout = z.infer<typeof payout>

/** `src/api/payout.rs` — `BalanceView`. Not documented — see above. */
export const payoutBalance = z.object({
  amount: z.string(),
  currency_code: z.string(),
})
export type PayoutBalance = z.infer<typeof payoutBalance>

/**
 * `src/api/admin_rest.rs`'s `run_state` — the six states a run itself moves
 * through. Not documented — see above.
 */
export const workflowRunState = z.enum([
  "running",
  "compensating",
  "waiting",
  "done",
  "reverted",
  "failed",
])
export type WorkflowRunState = z.infer<typeof workflowRunState>

/** `src/api/admin_rest.rs` — `WorkflowRunSummaryView`. Not documented — see above. */
export const workflowRunSummary = z.object({
  id: z.string(),
  name: z.string(),
  transaction_key: z.string(),
  state: workflowRunState,
  failure: z.string().nullable(),
  created_at: z.string(),
  finished_at: z.string().nullable(),
})
export type WorkflowRunSummary = z.infer<typeof workflowRunSummary>

/** `src/api/admin_rest.rs` — `WorkflowRunView`. Not documented — see above. */
export const workflowRun = z.object({
  id: z.string(),
  state: workflowRunState,
  failure: z.string().nullable(),
})
export type WorkflowRun = z.infer<typeof workflowRun>

/**
 * `src/workflow.rs` — the nine states a single step moves through, `called`
 * and `waiting` split out from `pending` for the two-phase steps that ask
 * something outside the run to call back. `skipped` is not a failure: it is
 * `Outcome::skipped`, a step that had nothing to do, and carries no
 * `compensate` when the run unwinds. `compensating` and `reverted` are the
 * trace of that unwind on a step that did write something.
 */
export const workflowStepState = z.enum([
  "pending",
  "invoking",
  "called",
  "waiting",
  "done",
  "skipped",
  "compensating",
  "reverted",
  "failed",
])
export type WorkflowStepState = z.infer<typeof workflowStepState>

/**
 * `src/api/admin_rest.rs` — `WorkflowStepView`. Not `output`: that is what
 * the runner needs to resume a step, not what a person debugging one reads.
 * Not documented — see above.
 */
export const workflowStep = z.object({
  id: z.string(),
  name: z.string(),
  ordering: z.number().int(),
  group_ordering: z.number().int(),
  state: workflowStepState,
  attempts: z.number().int(),
  max_attempts: z.number().int(),
  failure: z.string().nullable(),
  run_after: z.string(),
  lease_until: z.string().nullable(),
})
export type WorkflowStep = z.infer<typeof workflowStep>

/** `src/api/admin_rest.rs` — `WorkflowDeadLetterView`. Not documented — see above. */
export const workflowDeadLetter = z.object({
  id: z.string(),
  run_id: z.string(),
  step_name: z.string(),
  failure: z.string(),
  created_at: z.string(),
})
export type WorkflowDeadLetter = z.infer<typeof workflowDeadLetter>

/** `src/api/order_basket.rs` — `BasketView`. Not documented — see above. */
export const orderBasket = z.object({
  id: z.string(),
  display_id: z.number().int().nullable(),
  customer_id: z.string().nullable(),
  currency_code: z.string(),
  payment_collection_id: z.string().nullable(),
  email: z.string().nullable(),
  completed_at: z.string().nullable(),
})
export type OrderBasket = z.infer<typeof orderBasket>

/**
 * `src/api/store.rs` — `CartView`. `GET /admin/order-baskets/{id}/carts` is
 * the only admin route this panel reads it from; not documented — see above.
 */
export const cart = z.object({
  id: z.string(),
  customer_id: z.string().nullable(),
  email: z.string().nullable(),
  region_id: z.string().nullable(),
  currency_code: z.string(),
  completed_at: z.string().nullable(),
})
export type Cart = z.infer<typeof cart>

// — fulfilment, tax, pricing, payment, credit and carts: the same two halves
// as above, generated where #202 covers the operation, hand-written where it
// does not. A list whose item shape orval inlines into the page response
// rather than exporting on its own (`fulfilmentSet`, `shippingOptionType`,
// `refundReason`, `price`) is transcribed from that inline shape by hand,
// the same discipline as the fully undocumented ones below it.

export const fulfilmentProvider = GetAdminFulfillmentProvidersResponseItem
export type FulfilmentProvider = z.infer<typeof fulfilmentProvider>

/** `GET /admin/fulfillment-sets` — item embedded in the page response, not exported on its own. */
export const fulfilmentSet = z.object({
  id: z.string(),
  name: z.string(),
  kind: z.string(),
  created_at: z.string(),
})
export type FulfilmentSet = z.infer<typeof fulfilmentSet>

export const shippingOption = GetAdminShippingOptionsByIdResponse
export type ShippingOption = z.infer<typeof shippingOption>

/** `GET /admin/shipping-option-types` — item embedded in the page response, not exported on its own. */
export const shippingOptionType = z.object({
  id: z.string(),
  code: z.string(),
  label: z.string(),
  description: z.string().nullable(),
  created_at: z.string(),
})
export type ShippingOptionType = z.infer<typeof shippingOptionType>

export const shippingProfile = GetAdminShippingProfilesByIdResponse
export type ShippingProfile = z.infer<typeof shippingProfile>

export const taxRate = GetAdminTaxRatesByIdResponse
export type TaxRate = z.infer<typeof taxRate>

export const taxRegion = GetAdminTaxRegionsByIdResponse
export type TaxRegion = z.infer<typeof taxRegion>

export const taxRegistration = GetAdminTaxRegistrationsResponseItem
export type TaxRegistration = z.infer<typeof taxRegistration>

export const priceList = GetAdminPriceListsByIdResponse
export type PriceList = z.infer<typeof priceList>

/**
 * `GET /admin/price-preferences?attribute=...` — `src/api/admin_catalogue.rs`'s
 * `FindPricePreference` wants `attribute` (and takes an optional `value`),
 * neither declared in the document's `parameters` even though the handler
 * requires the first — reached by hand through `get()`'s own `query`, the
 * same gap `PayoutsLayout`'s lookups already work around.
 */
export const pricePreference = GetAdminPricePreferencesResponse
export type PricePreference = z.infer<typeof pricePreference>

export const priceSet = GetAdminPriceSetsByIdResponse
export type PriceSet = z.infer<typeof priceSet>

/** `GET /admin/price-sets/{id}/prices` — item embedded in the page response, not exported on its own. */
export const price = z.object({
  id: z.string(),
  amount: z.string(),
  currency_code: z.string(),
  price_set_id: z.string(),
  price_list_id: z.string().nullable(),
  min_quantity: z.number().int().nullable(),
  max_quantity: z.number().int().nullable(),
  rules_count: z.number().int(),
  title: z.string().nullable(),
  created_at: z.string(),
})
export type Price = z.infer<typeof price>

export const payment = GetAdminPaymentsByIdResponse
export type Payment = z.infer<typeof payment>

/** `GET /admin/refund-reasons` — item embedded in the page response, not exported on its own. */
export const refundReason = z.object({
  id: z.string(),
  code: z.string(),
  label: z.string(),
  description: z.string().nullable(),
  created_at: z.string(),
})
export type RefundReason = z.infer<typeof refundReason>

export const giftCard = GetAdminGiftCardsByIdResponse
export type GiftCard = z.infer<typeof giftCard>

/** `GET /admin/orders/{id}/entitlements` — what an order's money bought the right to. */
export const entitlement = GetAdminOrdersByIdEntitlementsResponseItem
export type Entitlement = z.infer<typeof entitlement>

export const revokeEntitlements = PostAdminOrdersByIdEntitlementsRevokeBody
export type RevokeEntitlements = z.infer<typeof revokeEntitlements>

/** `GET /admin/variants/{id}/digital-content` — a file a variant carries. */
export const digitalContent = GetAdminVariantsByIdDigitalContentResponseItem
export type DigitalContent = z.infer<typeof digitalContent>

export const createDigitalContent = PostAdminVariantsByIdDigitalContentBody
export type CreateDigitalContent = z.infer<typeof createDigitalContent>
