import { useQuery, type UseQueryResult } from "@tanstack/react-query"
import { useState } from "react"
import type { z } from "zod"

import { get, type ApiPath } from "@/api/client"
import { page, type Page } from "@/api/schemas"

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
 * The API hands out a cursor for the next page and nothing that walks back,
 * so "back" needs its own memory of the cursors already used — but that
 * memory is deliberately not what decides where the list is now. `after` and
 * `onAfterChange` are the caller's (a route's `validateSearch` and
 * `navigate`), so the page a screen shows is always the one its own URL
 * names — `/products?status=draft&after=...` is a page this hook did not
 * have to be on screen to reach.
 *
 * Changing a filter clears the stack: a cursor names a row in the ordering it
 * was issued under and means nothing in another.
 */
export function usePagedList<S extends z.ZodTypeAny>(
  key: unknown[],
  path: ApiPath,
  item: S,
  options: {
    after: string | undefined
    onAfterChange: (after: string | undefined) => void
    query?: Record<string, string | number | undefined>
    /** Fills any `{name}` `path` itself carries — a basket's own sub-lists. */
    params?: Record<string, string>
  }
): PagedList<z.infer<S>> {
  const { after, onAfterChange, query = {}, params = {} } = options
  const [backStack, setBackStack] = useState<string[]>([])

  const result = useQuery({
    queryKey: [...key, after],
    queryFn: ({ signal }) =>
      get(path, {
        signal,
        schema: page(item),
        params,
        query: { limit: 25, after, ...query },
      }),
  })

  return {
    result: result as UseQueryResult<Page<z.infer<S>>>,
    hasPrevious: after !== undefined,
    reset: () => {
      setBackStack([])
      onAfterChange(undefined)
    },
    back: () => {
      const previous = backStack.at(-1)
      setBackStack((stack) => stack.slice(0, -1))
      onAfterChange(previous)
    },
    forward: (cursor: string) => {
      setBackStack((stack) => (after !== undefined ? [...stack, after] : stack))
      onAfterChange(cursor)
    },
  }
}
