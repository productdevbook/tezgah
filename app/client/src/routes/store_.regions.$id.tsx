import { createFileRoute } from "@tanstack/react-router"

import { RegionDetail } from "@/features/store/region-detail"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.regions.new.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/regions/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <RegionDetail id={id} />
}
