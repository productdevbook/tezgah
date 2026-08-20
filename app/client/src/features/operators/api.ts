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
export const role = z.enum(["owner", "staff", "viewer"])

export type Role = z.infer<typeof role>

/**
 * What each may ask for, said here because the screen has to explain it and
 * the server is where it is enforced — `app/server/src/identity.rs`. An owner
 * may do anything and is the only one who may make an account; staff may run
 * the shop but not move money; a viewer may read.
 */
export const ROLE_MEANS: Record<Role, string> = {
  owner: "Anything, including making and disabling accounts",
  staff: "The shop's day-to-day. Not capturing, refunding or cancelling",
  viewer: "Reading, and nothing else",
}

export const operator = z.object({
  id: z.string(),
  email: z.string(),
  name: z.string(),
  role,
  created_at: z.string(),
  disabled_at: z.string().nullable(),
})

export type Operator = z.infer<typeof operator>

export const newOperator = z.object({
  email: z.string().trim().email("that is not an e-mail address"),
  name: z.string().trim().min(1, "a name is needed"),
  password: z.string().min(12, "a password is at least twelve characters"),
  role,
})

export type NewOperator = z.infer<typeof newOperator>

/** `null` when the caller is holding `ADMIN_TOKEN`, which is not a person. */
export const whoami = operator
  .pick({ id: true, email: true, name: true, role: true })
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

export async function patchOperator(
  id: string,
  patch: { disabled?: boolean; role?: Role }
): Promise<void> {
  await apiMutator(`/admin/operators/${encodeURIComponent(id)}`, {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(patch),
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

/**
 * An owner setting somebody else's password, which is what a shop does when
 * an operator forgets theirs — there is no reset e-mail because this server
 * has no mailer, and a link it cannot send would be worse than none.
 *
 * Every session that operator holds ends with it, including the one they may
 * be sitting in.
 */
export async function resetPassword(
  id: string,
  password: string
): Promise<void> {
  await apiMutator(`/admin/operators/${encodeURIComponent(id)}/password`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ password }),
  })
}
