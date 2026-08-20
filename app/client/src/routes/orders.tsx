import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Orders } from "@/features/orders/screen"

const ordersSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/orders")({
  validateSearch: ordersSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Orders
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
