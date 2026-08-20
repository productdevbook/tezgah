import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"

/**
 * The export's row and the import's are the same columns on purpose — a
 * shop's way of changing four hundred prices is to take the page out, edit it
 * and put it back, and that only works if what comes out goes in.
 *
 * Transcribed from `admin_catalogue::ProductExportView` and `ImportRow`
 * rather than generated: the document carries no body schema for either yet.
 */
export const exportRow = z.object({
  product_id: z.string(),
  handle: z.string(),
  product_title: z.string(),
  status: z.string(),
  variant_id: z.string(),
  variant_title: z.string(),
  sku: z.string().nullable(),
  price_amount: z.string().nullable(),
  price_currency: z.string().nullable(),
})

export type ExportRow = z.infer<typeof exportRow>

export const exportPage = z.object({
  items: z.array(exportRow),
  next: z.string().nullable(),
})

export const rejection = z.object({ row: z.number(), reason: z.string() })

export const importResult = z.object({
  created: z.number().optional(),
  updated: z.number().optional(),
  deleted: z.number().optional(),
  applied: z.number().optional(),
  rejected: z.array(rejection),
})

export type ImportResult = z.infer<typeof importResult>

/** The columns, in the order the CSV writes and reads them. */
export const COLUMNS = [
  "handle",
  "title",
  "subtitle",
  "description",
  "status",
  "variant_title",
  "sku",
  "price_amount",
  "price_currency",
] as const

export async function exportProducts(
  after: string | undefined,
  currency: string | undefined,
  signal?: AbortSignal
) {
  const params = new URLSearchParams()
  if (after) params.set("after", after)
  if (currency) params.set("currency_code", currency)
  const suffix = params.toString() ? `?${params}` : ""

  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    `/admin/products/export${suffix}`,
    { method: "GET", signal }
  )
  return parseResponse(exportPage, data, status)
}

export async function importProducts(rows: unknown[]): Promise<ImportResult> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/admin/products/batch",
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ rows, delete: [] }),
    }
  )
  return parseResponse(importResult, data, status)
}
