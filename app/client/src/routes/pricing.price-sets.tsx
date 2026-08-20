import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { PriceSets } from "@/features/pricing/price-sets"

const priceSetsSearch = z.object({ id: z.string().optional() })

export const Route = createFileRoute("/pricing/price-sets")({
  validateSearch: priceSetsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <PriceSets
      id={id}
      onIdChange={(next) => void navigate({ search: (prev) => ({ ...prev, id: next }) })}
    />
  )
}
