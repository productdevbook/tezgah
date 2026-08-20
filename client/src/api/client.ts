import type { z } from "zod"

import { held } from "@/lib/token"

import type { paths } from "./schema"

/**
 * Where the admin API is served. tezgah is a library and ships no server, so
 * this points at whatever host mounted `api::routes()`.
 */
const BASE = import.meta.env.VITE_TEZGAH_API ?? "/api"

/**
 * Every path the generated document declares.
 *
 * This is the one thing `schema.d.ts` is good for: the document carries no
 * schemas, so it cannot say what an operation answers with, but it does say
 * exactly which paths exist — which stops a typo becoming a 404 nobody reads.
 * The payload type is the caller's, out of `views.ts`.
 */
export type ApiPath = keyof paths

/**
 * Why a request did not answer, kept apart from what it answered.
 *
 * `unreachable` is its own case rather than a status of zero: no host is
 * serving the API, which is the state this panel is written in, and a screen
 * that renders it as "nothing here yet" tells the reader something false.
 *
 * `unauthenticated` is kept apart from `denied` because the two are not the
 * same sentence to read. A 401 with no token held means this panel never
 * presented one; a 401 with a token held means the server looked at it and
 * said no. Telling an operator "the authorizer refused" when the panel simply
 * had nothing to send is a wrong answer that sends them to the wrong place.
 *
 * `drifted` is the one this panel needs most. Its types are transcribed from
 * `src/api/*.rs` by hand (#202), so the crate can change one without this
 * knowing. Parsing at the boundary turns that from a blank cell into a
 * refusal that names the field.
 */
export class ApiError extends Error {
  readonly kind: ApiErrorKind
  readonly status: number
  readonly code?: string

  constructor(
    kind: ApiErrorKind,
    status: number,
    message: string,
    code?: string,
  ) {
    super(message)
    this.name = "ApiError"
    this.kind = kind
    this.status = status
    this.code = code
  }
}

export type ApiErrorKind =
  | "unreachable"
  | "unauthenticated"
  | "denied"
  | "not_found"
  | "refused"
  | "drifted"

function kindOf(status: number, hadToken: boolean): ApiErrorKind {
  if (status === 401 && !hadToken) return "unauthenticated"
  if (status === 401 || status === 403) return "denied"
  if (status === 404) return "not_found"
  return "refused"
}

function fill(path: string, params: Record<string, string>): string {
  return path.replace(/\{(\w+)\}/g, (_, name: string) => {
    const value = params[name]
    if (value === undefined) throw new Error(`${path} wants ${name}`)
    return encodeURIComponent(value)
  })
}

export async function get<S extends z.ZodTypeAny>(
  path: ApiPath,
  options: {
    schema: S
    params?: Record<string, string>
    query?: Record<string, string | number | undefined>
    signal?: AbortSignal
  },
): Promise<z.infer<S>> {
  const url = new URL(BASE + fill(path, options.params ?? {}), location.origin)
  for (const [key, value] of Object.entries(options.query ?? {})) {
    if (value !== undefined) url.searchParams.set(key, String(value))
  }

  const token = held()
  const headers: Record<string, string> = { accept: "application/json" }
  if (token) headers.authorization = `Bearer ${token}`

  let response: Response
  try {
    response = await fetch(url, { signal: options.signal, headers })
  } catch {
    throw new ApiError("unreachable", 0, `no host answered at ${BASE}`)
  }

  if (!response.ok) {
    const body: unknown = await response.json().catch(() => null)
    const said = (body ?? {}) as { code?: string; message?: string }
    throw new ApiError(
      kindOf(response.status, token !== null),
      response.status,
      said.message ?? response.statusText,
      said.code,
    )
  }

  const parsed = options.schema.safeParse(await response.json())
  if (!parsed.success) {
    throw new ApiError(
      "drifted",
      response.status,
      parsed.error.issues
        .slice(0, 3)
        .map((i) => `${i.path.join(".") || "(root)"}: ${i.message}`)
        .join("; "),
    )
  }
  return parsed.data
}
