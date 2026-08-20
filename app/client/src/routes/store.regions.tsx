import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { StoreRegions } from "@/features/store/regions"

const regionsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/store/regions")({
  validateSearch: regionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <StoreRegions
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
