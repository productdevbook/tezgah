import type { UseQueryResult } from "@tanstack/react-query"
import { useT } from "@/panel/i18n"

import { ApiError } from "@/api/client"
import { panelRuntime } from "@/panel/runtime"
import { Button } from "@/components/ui/button"
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import { Skeleton } from "@/components/ui/skeleton"

/**
 * The three answers a screen can get that are not rows, told apart.
 *
 * "Nothing came back" and "nobody answered" render the same in most panels and
 * are not the same thing: tezgah ships no server, so an unreachable API is the
 * expected state here and saying "no products yet" instead would be a lie the
 * reader acts on.
 */
export function QueryState<T>({
  query,
  empty,
  children,
}: {
  query: UseQueryResult<T>
  empty: { title: string; description: string }
  children: (data: T) => React.ReactNode
}) {
  if (query.isPending) {
    return (
      <div className="space-y-2" aria-busy="true">
        {Array.from({ length: 6 }, (_, i) => (
          <Skeleton key={i} className="h-12 w-full" />
        ))}
      </div>
    )
  }

  if (query.isError) {
    return <Failure error={query.error} onRetry={() => void query.refetch()} />
  }

  const data = query.data
  if (Array.isArray(data) && data.length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyTitle>{empty.title}</EmptyTitle>
          <EmptyDescription>{empty.description}</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }

  return <>{children(data)}</>
}

function Failure({ error, onRetry }: { error: unknown; onRetry: () => void }) {
  const t = useT()
  const api = error instanceof ApiError ? error : undefined

  const said = {
    unreachable: {
      title: t("state.noHost"),
      description: t("state.noHostWhy"),
    },
    unauthenticated: {
      title: t("state.noToken"),
      description: t("state.noTokenWhy"),
    },
    denied: {
      title: t("state.refused"),
      description: t("state.refusedWhy"),
    },
    not_found: {
      title: t("state.notHere"),
      description: t("state.notHereWhy"),
    },
    refused: {
      title: t("state.refusedRequest"),
      description: api?.message ?? t("error.refused"),
    },
    drifted: {
      title: t("state.drifted"),
      description: t("state.driftedWhy"),
    },
  }[api?.kind ?? "refused"]

  return (
    <Empty>
      <EmptyHeader>
        <EmptyTitle>{said.title}</EmptyTitle>
        <EmptyDescription>{said.description}</EmptyDescription>
      </EmptyHeader>
      <EmptyContent>
        <div className="flex flex-col items-center gap-2">
          {api?.kind === "drifted" ? (
            <code className="max-w-lg rounded bg-muted px-2 py-1 text-left text-xs break-words">
              {api.message}
            </code>
          ) : null}
          <div className="flex items-center gap-2">
            {api?.kind === "unauthenticated" || api?.kind === "denied" ? (
              <Button
                size="sm"
                onClick={() => panelRuntime().onUnauthenticated()}
              >
                Connect
              </Button>
            ) : null}
            <Button size="sm" variant="outline" onClick={onRetry}>
              Try again
            </Button>
            {api?.code ? (
              <code className="text-xs text-muted-foreground">{api.code}</code>
            ) : null}
          </div>
        </div>
      </EmptyContent>
    </Empty>
  )
}
