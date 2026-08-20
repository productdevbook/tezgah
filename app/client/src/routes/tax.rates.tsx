import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { TaxRates } from "@/features/tax/rates"

const ratesSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/tax/rates")({
  validateSearch: ratesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <TaxRates
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
