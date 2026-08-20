import { createFileRoute } from "@tanstack/react-router"

import { ShippingProfileDetail } from "@/features/fulfilment/shipping-profile-detail"

/** A full page, not a `/fulfilment` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/fulfilment_/shipping-profiles/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <ShippingProfileDetail id={id} />
}
