import { createFileRoute } from "@tanstack/react-router"

import { SalesChannelDetail } from "@/features/store/sales-channel-detail"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.sales-channels.new.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/sales-channels/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <SalesChannelDetail id={id} />
}
