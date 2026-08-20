import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Payments } from "@/features/payments/payments"

const paymentsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/payments/")({
  validateSearch: paymentsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Payments
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
