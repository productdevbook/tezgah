import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { ShippingOptionTypes } from "@/features/fulfilment/shipping-option-types"

const shippingOptionTypesSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/fulfilment/shipping-option-types")({
  validateSearch: shippingOptionTypesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <ShippingOptionTypes
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
