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
 * `unreachable`, `unauthenticated`, `denied`, `not_found` and `refused` are
 * decided from the status code and whether a token was sent — `mutator.ts`
 * decides them, and nothing here needs a schema to do it.
 *
 * `drifted` is the one exception: it needs a schema to know a response is
 * wrong, so it is thrown one layer up, in `drift.ts`, after the mutator has
 * already returned a 2xx.
 */
export class ApiError extends Error {
  readonly kind: ApiErrorKind
  readonly status: number
  readonly code?: string

  constructor(kind: ApiErrorKind, status: number, message: string, code?: string) {
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

export function kindOf(status: number, hadToken: boolean): ApiErrorKind {
  if (status === 401 && !hadToken) return "unauthenticated"
  if (status === 401 || status === 403) return "denied"
  if (status === 404) return "not_found"
  return "refused"
}

/**
 * `server/src/http/mod.rs`'s `ApiError` answers `{"error": {"code",
 * "message"}}`, never the two fields at the top level.
 */
export async function said(response: Response): Promise<[message: string, code?: string]> {
  const body: unknown = await response.json().catch(() => null)
  const fields = ((body as { error?: unknown } | null)?.error ?? {}) as {
    code?: string
    message?: string
  }
  return [fields.message ?? response.statusText, fields.code]
}
