import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"

/**
 * What `POST /admin/inventory-items/batch` takes and answers, from the
 * document — `SetStockLevelsBody` and `BatchResultView` describe themselves
 * now, so nothing here is transcribed out of the Rust.
 */
export const stockLevelRow = z.object({
  inventory_item_id: z.string(),
  location_id: z.string(),
  stocked_quantity: z.number().int(),
  incoming_quantity: z.number().int().nullable().optional(),
})

export type StockLevelRow = z.infer<typeof stockLevelRow>

export const batchResult = z.object({
  applied: z.number(),
  rejected: z.array(z.object({ row: z.number(), reason: z.string() })),
})

export type BatchResult = z.infer<typeof batchResult>

export async function saveStockLevels(
  levels: StockLevelRow[]
): Promise<BatchResult> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/admin/inventory-items/batch",
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ levels }),
    }
  )
  return parseResponse(batchResult, data, status)
}
