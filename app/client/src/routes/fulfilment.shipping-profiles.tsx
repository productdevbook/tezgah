import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { ShippingProfiles } from "@/features/fulfilment/shipping-profiles"

const shippingProfilesSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/fulfilment/shipping-profiles")({
  validateSearch: shippingProfilesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <ShippingProfiles
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
