import { createFileRoute } from "@tanstack/react-router"

import { PriceListDetail } from "@/features/pricing/price-list-detail"

/** A full page, not a `/pricing` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/pricing_/price-lists/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <PriceListDetail id={id} />
}
