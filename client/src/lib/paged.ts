import { useQuery, type UseQueryResult } from "@tanstack/react-query"
import { useState } from "react"
import type { z } from "zod"

import { get, type ApiPath } from "@/api/client"
import { page, type Page } from "@/api/views"

export type PagedList<T> = {
  result: UseQueryResult<Page<T>>
  hasPrevious: boolean
  reset: () => void
  back: () => void
  forward: (cursor: string) => void
}

/**
 * A list that walks forward by cursor and remembers how it got here.
 *
 * The API hands out a cursor for the next page and nothing that walks back, so
 * "back" is the stack of cursors already used rather than an offset that could
 * be computed. Changing a filter clears it: a cursor names a row in the
 * ordering it was issued under and means nothing in another.
 */
export function usePagedList<S extends z.ZodTypeAny>(
  key: unknown[],
  path: ApiPath,
  item: S,
  query: Record<string, string | number | undefined> = {},
): PagedList<z.infer<S>> {
  const [cursors, setCursors] = useState<string[]>([])
  const after = cursors.at(-1)

  const result = useQuery({
    queryKey: [...key, after],
    queryFn: ({ signal }) =>
      get(path, {
        signal,
        schema: page(item),
        query: { limit: 25, after, ...query },
      }),
  })

  return {
    result: result as UseQueryResult<Page<z.infer<S>>>,
    hasPrevious: cursors.length > 0,
    reset: () => setCursors([]),
    back: () => setCursors((c) => c.slice(0, -1)),
    forward: (cursor: string) => setCursors((c) => [...c, cursor]),
  }
}
