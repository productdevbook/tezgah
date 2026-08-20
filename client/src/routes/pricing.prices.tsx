import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Prices } from "@/features/pricing/prices"

const pricesSearch = z.object({
  priceSet: z.string().optional(),
  after: z.string().optional(),
})

export const Route = createFileRoute("/pricing/prices")({
  validateSearch: pricesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { priceSet, after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Prices
      priceSetId={priceSet}
      onPriceSetIdChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, priceSet: next, after: undefined }) })
      }
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
