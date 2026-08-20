import { useQuery } from "@tanstack/react-query"

import { whoAmI, type Role, type whoami } from "@/features/operators/api"
import type { z } from "zod"

export type WhoAmI = z.infer<typeof whoami>

/**
 * Who is holding this browser's token, asked once and shared.
 *
 * `null` is a real answer rather than a missing one: an `ADMIN_TOKEN` holder
 * is not a person, and `GET /auth/me` says so. Everything asking "may they"
 * has to read that as yes — the shared secret is how a shop that lost every
 * password gets back in, so it can do whatever an owner can.
 */
export function useWhoAmI() {
  return useQuery({
    queryKey: ["whoami"],
    queryFn: ({ signal }) => whoAmI(signal),
    staleTime: 5 * 60 * 1000,
  })
}

/** The two screens `only_an_owner` refuses in `app/server/src/http/auth.rs`. */
const OWNER_ONLY = new Set(["operators", "records"])

/**
 * This hides a door; it does not lock one. The lock is on the server, and a
 * URL typed by hand still reaches it and is still refused.
 */
export function mayOpen(slug: string, who: WhoAmI | undefined): boolean {
  if (!OWNER_ONLY.has(slug)) return true
  // `undefined` is the answer still in flight, and `null` is the admin token.
  // Both draw the section: a sidebar that rearranges itself a moment after it
  // appears is worse than one that lets the server say no.
  if (who === undefined || who === null) return true
  return who.role === ("owner" satisfies Role)
}
