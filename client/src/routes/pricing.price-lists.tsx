import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { PriceLists } from "@/features/pricing/price-lists"

const priceListsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/pricing/price-lists")({
  validateSearch: priceListsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <PriceLists
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
