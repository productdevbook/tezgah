import type { z } from "zod"

import { apiMutator } from "@/api/mutator"
import { parseResponse } from "@/api/drift"

import type { paths } from "./schema"

export { ApiError, type ApiErrorKind } from "@/api/errors"

/**
 * Every path the generated document declares.
 *
 * This is the one thing `schema.d.ts` is good for: the document carries no
 * schemas for most of its operations, so it cannot say what every one
 * answers with, but it does say exactly which paths exist — which stops a
 * typo becoming a 404 nobody reads. The payload type is the caller's, out of
 * `api/schemas.ts`.
 */
export type ApiPath = keyof paths

function fill(path: string, params: Record<string, string>): string {
  return path.replace(/\{(\w+)\}/g, (_, name: string) => {
    const value = params[name]
    if (value === undefined) throw new Error(`${path} wants ${name}`)
    return encodeURIComponent(value)
  })
}

function withQuery(
  path: string,
  query?: Record<string, string | number | undefined>
): string {
  if (!query) return path
  const params = new URLSearchParams()
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined) params.set(key, String(value))
  }
  const search = params.toString()
  return search ? `${path}?${search}` : path
}

/**
 * `get`/`post` for the paths #202 has not documented a body for — every
 * write form still builds its own request from a hand-written schema in
 * `api/schemas.ts`. Where a response is documented, the orval-generated
 * fetch functions in `api/generated/fetch/**` call `apiMutator` directly and
 * do not go through these; a screen that wants the drift check on a
 * generated response calls `parseResponse` itself, the same way these two
 * do.
 */
export async function get<S extends z.ZodTypeAny>(
  path: ApiPath,
  options: {
    schema: S
    params?: Record<string, string>
    query?: Record<string, string | number | undefined>
    signal?: AbortSignal
  }
): Promise<z.infer<S>> {
  const url = withQuery(fill(path, options.params ?? {}), options.query)
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(url, {
    method: "GET",
    signal: options.signal,
  })
  return parseResponse(options.schema, data, status)
}

export async function post<S extends z.ZodTypeAny>(
  path: ApiPath,
  options: {
    schema: S
    body: unknown
    params?: Record<string, string>
    signal?: AbortSignal
  }
): Promise<z.infer<S>> {
  const url = fill(path, options.params ?? {})
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(url, {
    method: "POST",
    signal: options.signal,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(options.body),
  })
  return parseResponse(options.schema, data, status)
}

export async function patch<S extends z.ZodTypeAny>(
  path: ApiPath,
  options: {
    schema: S
    body: unknown
    params?: Record<string, string>
    signal?: AbortSignal
  }
): Promise<z.infer<S>> {
  const url = fill(path, options.params ?? {})
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(url, {
    method: "PATCH",
    signal: options.signal,
    headers: { "content-type": "application/json" },
    body: JSON.stringify(options.body),
  })
  return parseResponse(options.schema, data, status)
}

/**
 * Every bound `DELETE` answers with an undocumented body (`zod.unknown()` in
 * the generated zod, when it has one at all) — there is nothing here for
 * `drift.ts` to check a screen against, so this resolves once `apiMutator`
 * has confirmed the status was 2xx rather than parsing a response.
 */
export async function del(
  path: ApiPath,
  options: { params?: Record<string, string>; signal?: AbortSignal }
): Promise<void> {
  const url = fill(path, options.params ?? {})
  await apiMutator<{ data: unknown; status: number }>(url, {
    method: "DELETE",
    signal: options.signal,
  })
}
