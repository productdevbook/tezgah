import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { StoreSalesChannels } from "@/features/store/sales-channels"

const channelsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/store/sales-channels")({
  validateSearch: channelsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <StoreSalesChannels
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
