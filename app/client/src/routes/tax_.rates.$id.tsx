import { createFileRoute } from "@tanstack/react-router"

import { TaxRateDetail } from "@/features/tax/rate-detail"

/** A full page, not a `/tax` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/tax_/rates/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <TaxRateDetail id={id} />
}
