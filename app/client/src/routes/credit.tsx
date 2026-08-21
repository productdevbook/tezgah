import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { GiftCards } from "@/features/credit/screen"

const creditSearch = z.object({
  after: z.string().optional(),
  state: z.enum(["live", "disabled", "spent"]).optional(),
})

export const Route = createFileRoute("/credit")({
  validateSearch: creditSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, state } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <GiftCards
      after={after}
      state={state ?? "all"}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
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
