import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { TaxRegions } from "@/features/tax/regions"

const regionsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/tax/regions")({
  validateSearch: regionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <TaxRegions
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
