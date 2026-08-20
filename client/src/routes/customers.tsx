import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Customers } from "@/features/customers/screen"

const customersSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/customers")({
  validateSearch: customersSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Customers
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
