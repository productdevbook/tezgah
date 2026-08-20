import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Subscriptions } from "@/features/subscriptions/screen"

const subscriptionsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/subscriptions")({
  validateSearch: subscriptionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Subscriptions
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
