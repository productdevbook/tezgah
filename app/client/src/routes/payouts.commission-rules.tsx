import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { CommissionRules } from "@/features/payouts/commission-rules"

const commissionRulesSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/payouts/commission-rules")({
  validateSearch: commissionRulesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <CommissionRules
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
