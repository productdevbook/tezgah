import { Outlet, createFileRoute } from "@tanstack/react-router"

import { SalesChannelDetail } from "@/features/store/sales-channel-detail"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.sales-channels.new.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/sales-channels/$id")({
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what `/store/sales-channels/$id/edit` draws into. Without it that address
 * resolved to this page and the edit form was never rendered — a child route
 * whose parent draws no outlet is a screen nothing can reach.
 */
export function RouteComponent() {
  const { id } = Route.useParams()
  return (
    <>
      <SalesChannelDetail id={id} />
      <Outlet />
    </>
  )
}
