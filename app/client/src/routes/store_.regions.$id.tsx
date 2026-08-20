import { Outlet, createFileRoute } from "@tanstack/react-router"

import { RegionDetail } from "@/features/store/region-detail"

/**
 * A full page, not a `/store` tab's content — same reasoning as
 * `store_.regions.new.tsx`, and the same underscore for it.
 */
export const Route = createFileRoute("/store_/regions/$id")({
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what `/store/regions/$id/edit` draws into. Without it that address
 * resolved to this page and the edit form was never rendered — a child route
 * whose parent draws no outlet is a screen nothing can reach.
 */
export function RouteComponent() {
  const { id } = Route.useParams()
  return (
    <>
      <RegionDetail id={id} />
      <Outlet />
    </>
  )
}
