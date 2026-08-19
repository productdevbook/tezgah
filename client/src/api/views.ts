/**
 * What the admin surface actually answers with.
 *
 * **Transcribed from the Rust by hand**, and that is not the intended state.
 * `tests/snapshots/openapi.json` declares 483 operations and zero schemas —
 * no request bodies, no response bodies, no `components/schemas` — so
 * `schema.d.ts`, generated from it, types the paths and leaves every payload
 * `unknown`. Until the generator emits schemas these will drift silently, and
 * only a reader comparing them to `src/api/*.rs` will notice. See the issue
 * this file is named in.
 *
 * Each type below names the Rust struct it mirrors so the comparison is one
 * grep, not a search.
 */

/** `src/page.rs` — `Page<T>`. `next` is the cursor, absent on the last page. */
export type Page<T> = {
  items: T[]
  next: string | null
}

/** `src/catalogue.rs` — `ProductStatus`, serialized lowercase. */
export type ProductStatus =
  | "draft"
  | "proposed"
  | "published"
  | "archived"
  | "rejected"

/** `src/api/admin_catalogue.rs` — `ProductView`. */
export type Product = {
  id: string
  handle: string
  title: string
  subtitle: string | null
  description: string | null
  status: ProductStatus
  rejected_reason: string | null
  thumbnail_url: string | null
  is_discountable: boolean
  product_type_id: string | null
  product_collection_id: string | null
  weight: string | null
  length: string | null
  height: string | null
  width: string | null
  material: string | null
  hs_code: string | null
  origin_country: string | null
  external_id: string | null
}

/** `src/api/admin_order.rs` — `OrderView`. */
export type Order = {
  id: string
  display_id: number | null
  email: string | null
  currency_code: string
  version: number
  status: string
  payment_status: string
  fulfillment_status: string
  is_draft: boolean
  payment_collection_id: string | null
  basket_id: string | null
  completed_at: string | null
  canceled_at: string | null
  created_at: string
}

/** `src/api/admin_catalogue.rs` — `InventoryItemView`. */
export type InventoryItem = {
  id: string
  sku: string | null
  title: string | null
  requires_shipping: boolean
  created_at: string
}

/** `src/api/admin_catalogue.rs` — `ListProducts`. */
export type ListProducts = {
  after?: string
  limit?: number
  status?: ProductStatus
  collection?: string
  product_type?: string
  category?: string
  tag?: string
}

/** `src/api/admin_order.rs` — `ListOrders`. */
export type ListOrders = {
  after?: string
  limit?: number
  customer_id?: string
}

/** The paging half every `ListQuery` in `src/api/` carries. */
export type ListQuery = {
  after?: string
  limit?: number
}
