import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Subscriptions } from "@/features/subscriptions/screen"

const subscriptionsSearch = z.object({
  after: z.string().optional(),
  status: z
    .enum(["active", "past_due", "cancelled", "expired", "paused"])
    .optional(),
  ending: z.enum(["ending", "staying"]).optional(),
})

export const Route = createFileRoute("/subscriptions")({
  validateSearch: subscriptionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, status, ending } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Subscriptions
      after={after}
      status={status ?? "all"}
      ending={ending ?? "all"}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
      onStatusChange={(next) =>
        // The cursor goes with it: it names a row in the ordering it was
        // issued under and means nothing under another filter.
        void navigate({
          search: (prev) => ({
            ...prev,
            status: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
      onEndingChange={(next) =>
        void navigate({
          search: (prev) => ({
            ...prev,
            ending: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
    />
  )
}
