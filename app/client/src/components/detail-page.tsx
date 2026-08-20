import type { UseQueryResult } from "@tanstack/react-query"
import type { ReactNode } from "react"

import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { TwoColumnPage } from "@/components/two-column"

/**
 * A record's page, the same shape for every record there is.
 *
 * Loading, refusal and drift come from `QueryState`; going back comes from
 * `DetailHeader`; what is left for a screen to decide is what the record is
 * called, what can be done to it, and which facts belong together. `main` is
 * the record itself and `side` is what it belongs to — one column on a narrow
 * screen, in that order.
 *
 * `<Outlet />` goes below it: a route's children are drawn over this page as
 * drawers, not inside it.
 */
export function DetailPage<T>({
  query,
  empty,
  back,
  title,
  subtitle,
  actions,
  main,
  side,
}: {
  query: UseQueryResult<T>
  empty: { title: string; description: string }
  back: string
  title: (item: T) => string
  subtitle?: (item: T) => string | undefined
  actions?: (item: T) => ReactNode
  main: (item: T) => ReactNode
  side?: (item: T) => ReactNode
}) {
  return (
    <QueryState query={query} empty={empty}>
      {(item) => (
        <div className="flex flex-col gap-4">
          <DetailHeader
            back={back}
            title={title(item)}
            subtitle={subtitle?.(item)}
          >
            {actions?.(item)}
          </DetailHeader>
          <TwoColumnPage main={main(item)} side={side?.(item)} />
        </div>
      )}
    </QueryState>
  )
}
