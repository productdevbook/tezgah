import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Carts } from "@/features/carts/screen"

const cartsSearch = z.object({
  after: z.string().optional(),
  q: z.string().optional(),
  state: z.enum(["open", "completed"]).optional(),
})

export const Route = createFileRoute("/carts")({
  validateSearch: cartsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, q, state } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Carts
      after={after}
      q={q}
      state={state ?? "all"}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
      onQChange={(next) =>
        // The cursor goes with it: it names a row in the ordering it was
        // issued under and means nothing under another filter.
        void navigate({
          search: (prev) => ({ ...prev, q: next, after: undefined }),
        })
      }
      onStateChange={(next) =>
        void navigate({
          search: (prev) => ({
            ...prev,
            state: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
    />
  )
}
