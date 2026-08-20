import { createFileRoute } from "@tanstack/react-router"

import { EditSalesChannel } from "@/features/store/sales-channel-edit"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.sales-channels.$id.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/sales-channels/$id/edit")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditSalesChannel id={id} />
}
