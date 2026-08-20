import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { RefundReasons } from "@/features/payments/refund-reasons"

const refundReasonsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/payments/refund-reasons")({
  validateSearch: refundReasonsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <RefundReasons
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
