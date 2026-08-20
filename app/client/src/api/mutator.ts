import { ApiError, kindOf, said } from "@/api/errors"
import { panelRuntime } from "@/panel/runtime"

/**
 * The one place an admin request actually leaves the browser.
 *
 * Every orval-generated call (`api/generated/fetch/**`) is wired to this as
 * its `mutator`, and `api/client.ts`'s hand-rolled `get`/`post` — for the
 * paths #202 has not documented a body for yet — call it too, so there is
 * exactly one function that attaches the token and reads a status code.
 *
 * A custom mutator turns off orval's own response validation (it emits
 * `return mutator(url, options)` and nothing else), which is deliberate: four
 * of the five `ApiError` kinds need no schema at all, only a status code and
 * whether a token was sent, and deciding them here means `drift.ts` only has
 * to handle the one that does.
 *
 * What this returns is deliberately unchecked — `data` is whatever the body
 * parsed to, not validated against anything. A 2xx that is the wrong shape is
 * `drift.ts`'s problem, one layer up, where a schema is available to say so.
 *
 * Where the request goes and what it carries are `panel/runtime.ts`'s answers
 * rather than this module's: a generated fetch function is not a component,
 * so a context cannot reach here, and a host mounting these screens inside
 * its own back office has its own address and its own token.
 */
export async function apiMutator<T>(
  url: string,
  options?: RequestInit
): Promise<T> {
  const { apiBase: BASE, token: heldToken, onUnauthenticated } = panelRuntime()
  const token = heldToken()
  const headers = new Headers(options?.headers)
  headers.set("accept", "application/json")
  if (token) headers.set("authorization", `Bearer ${token}`)

  let response: Response
  try {
    response = await fetch(BASE + url, { ...options, headers })
  } catch {
    throw new ApiError("unreachable", 0, `no host answered at ${BASE}`)
  }

  if (!response.ok) {
    const kind = kindOf(response.status, token !== null)
    if (kind === "unauthenticated") onUnauthenticated()
    throw new ApiError(kind, response.status, ...(await said(response)))
  }

  const data =
    response.status === 204
      ? undefined
      : await response.json().catch(() => undefined)

  return { data, status: response.status, headers: response.headers } as T
}
