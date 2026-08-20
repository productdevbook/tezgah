import { useQuery, type UseQueryResult } from "@tanstack/react-query"
import type { z } from "zod"

import { get, type ApiPath } from "@/api/client"

/**
 * One record by id — `usePagedList`'s counterpart now that `GET .../{id}` is
 * bound. The route owns the id (`$id` in its own path); this only turns it
 * into the query, the same `get()` every list already goes through, so
 * `denied`, `not_found` and `drifted` fall out of `QueryState` without the
 * screen having to know about them.
 */
export function useDetail<S extends z.ZodTypeAny>(
  key: unknown[],
  path: ApiPath,
  item: S,
  id: string
): UseQueryResult<z.infer<S>> {
  return useQuery({
    queryKey: [...key, id],
    queryFn: ({ signal }) => get(path, { signal, schema: item, params: { id } }),
  })
}

export function dateTime(value: string): string {
  return new Date(value).toLocaleString()
}
