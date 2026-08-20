import { createFileRoute } from "@tanstack/react-router"

import { ShippingOptionDetail } from "@/features/fulfilment/shipping-option-detail"

/** A full page, not a `/fulfilment` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/fulfilment_/shipping-options/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <ShippingOptionDetail id={id} />
}
