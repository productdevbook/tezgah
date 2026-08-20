import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"

/**
 * What `POST /admin/prices/batch` takes, transcribed from
 * `admin_catalogue::PriceChangeRow` — the document carries no body schema for
 * it yet.
 *
 * An amount is a string on the way out and a string on the way in: tezgah
 * stores money as `NUMERIC` and never as a float, and turning it into a
 * JavaScript number here to turn it back would be the one place that lost a
 * hundredth of a lira.
 */
export const priceChange = z.object({
  id: z.string(),
  amount: z.string(),
  currency_code: z.string(),
})

export type PriceChange = z.infer<typeof priceChange>

export const batchResult = z.object({
  applied: z.number(),
  rejected: z.array(z.object({ row: z.number(), reason: z.string() })),
})

export type BatchResult = z.infer<typeof batchResult>

export async function savePrices(prices: PriceChange[]): Promise<BatchResult> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/admin/prices/batch",
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ prices }),
    }
  )
  return parseResponse(batchResult, data, status)
}
