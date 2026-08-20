import type { z } from "zod"

import { ApiError } from "@/api/errors"

/**
 * The boundary that decides `drifted`.
 *
 * `mutator.ts` hands back a 2xx with an unchecked body; this is what checks
 * it against a schema and turns a mismatch into an error that names the
 * field, instead of `undefined` sitting quietly in a cell. The schema is
 * generated (`api/generated/zod/**`, from `input.override.transformer`'s
 * copy of `tests/snapshots/openapi.json`) wherever #202 documents the shape,
 * and hand-written in `api/schemas.ts` everywhere it does not yet.
 */
export function parseResponse<S extends z.ZodTypeAny>(
  schema: S,
  data: unknown,
  status: number
): z.infer<S> {
  const parsed = schema.safeParse(data)
  if (!parsed.success) {
    throw new ApiError(
      "drifted",
      status,
      parsed.error.issues
        .slice(0, 3)
        .map((i) => `${i.path.join(".") || "(root)"}: ${i.message}`)
        .join("; ")
    )
  }
  return parsed.data
}
