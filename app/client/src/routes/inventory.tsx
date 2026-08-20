import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Inventory } from "@/features/inventory/screen"

const inventorySearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/inventory")({
  validateSearch: inventorySearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Inventory
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
