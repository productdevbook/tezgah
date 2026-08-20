import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"

/**
 * The server's own routes, not tezgah's.
 *
 * `api/client.ts`'s `get`/`post` take an `ApiPath` — one of the paths
 * `tests/snapshots/openapi.json` declares — and `/admin/operators` is not
 * one of them, because tezgah authenticates nobody and declares no route for
 * it. So these go through the same mutator and the same drift check, and say
 * plainly that their shapes are transcribed from `app/server/src/http/auth.rs`
 * rather than generated from a document.
 */
export const operator = z.object({
  id: z.string(),
  email: z.string(),
  name: z.string(),
  created_at: z.string(),
  disabled_at: z.string().nullable(),
})

export type Operator = z.infer<typeof operator>

export const newOperator = z.object({
  email: z.string().trim().email("that is not an e-mail address"),
  name: z.string().trim().min(1, "a name is needed"),
  password: z.string().min(12, "a password is at least twelve characters"),
})

export type NewOperator = z.infer<typeof newOperator>

/** `null` when the caller is holding `ADMIN_TOKEN`, which is not a person. */
export const whoami = operator
  .pick({ id: true, email: true, name: true })
  .nullable()

export async function listOperators(signal?: AbortSignal): Promise<Operator[]> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/admin/operators",
    { method: "GET", signal }
  )
  return parseResponse(z.array(operator), data, status)
}

export async function createOperator(body: NewOperator): Promise<void> {
  await apiMutator("/admin/operators", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  })
}

export async function setDisabled(
  id: string,
  disabled: boolean
): Promise<void> {
  await apiMutator(`/admin/operators/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ disabled }),
  })
}

export async function whoAmI(
  signal?: AbortSignal
): Promise<z.infer<typeof whoami>> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/auth/me",
    { method: "GET", signal }
  )
  return parseResponse(whoami, data, status)
}
