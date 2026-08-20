import { createFileRoute } from "@tanstack/react-router"

import { EditRegion } from "@/features/store/region-edit"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.regions.$id.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/regions/$id/edit")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditRegion id={id} />
}
