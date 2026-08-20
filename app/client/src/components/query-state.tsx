import type { UseQueryResult } from "@tanstack/react-query"

import { ApiError } from "@/api/client"
import { forget } from "@/lib/token"
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
  const api = error instanceof ApiError ? error : undefined

  const said = {
    unreachable: {
      title: "No host is answering",
      description:
        "tezgah is a library and serves nothing itself. Point VITE_TEZGAH_API at whatever mounts api::routes(), or run the example shop.",
    },
    unauthenticated: {
      title: "This panel has no token",
      description:
        "The admin surface wants one and nothing was sent. Connect the panel with the server's ADMIN_TOKEN.",
    },
    denied: {
      title: "Refused",
      description:
        "The token was sent and the server said no — wrong token, or one the server no longer holds. It never says which rows exist, so there is nothing more to read into that.",
    },
    not_found: {
      title: "Not here",
      description: "The host answered, and has no such row.",
    },
    refused: {
      title: "The request did not go through",
      description: api?.message ?? "The host answered with an error.",
    },
    drifted: {
      title: "The panel and the crate disagree",
      description:
        "The host answered, and the answer is not the shape this panel expects. Its types are transcribed from the Rust by hand, so the crate has moved and this has not.",
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
            <code className="bg-muted max-w-lg rounded px-2 py-1 text-left text-xs break-words">
              {api.message}
            </code>
          ) : null}
          <div className="flex items-center gap-2">
            {api?.kind === "unauthenticated" || api?.kind === "denied" ? (
              <Button
                size="sm"
                onClick={() => {
                  forget()
                  location.reload()
                }}
              >
                Connect
              </Button>
            ) : null}
            <Button size="sm" variant="outline" onClick={onRetry}>
              Try again
            </Button>
            {api?.code ? (
              <code className="text-muted-foreground text-xs">{api.code}</code>
            ) : null}
          </div>
        </div>
      </EmptyContent>
    </Empty>
  )
}
