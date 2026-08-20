import { Outlet, createFileRoute } from "@tanstack/react-router"

import { CustomerDetail } from "@/features/customers/detail"

export const Route = createFileRoute("/customers_/$id")({
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what `/customers/$id/edit` draws into. Without it that address
 * resolved to this page and the edit form was never rendered — a child route
 * whose parent draws no outlet is a screen nothing can reach.
 */
export function RouteComponent() {
  const { id } = Route.useParams()
  return (
    <>
      <CustomerDetail id={id} />
      <Outlet />
    </>
  )
}
