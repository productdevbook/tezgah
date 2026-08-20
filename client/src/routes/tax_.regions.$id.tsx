import { createFileRoute } from "@tanstack/react-router"

import { TaxRegionDetail } from "@/features/tax/region-detail"

/** A full page, not a `/tax` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/tax_/regions/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <TaxRegionDetail id={id} />
}
