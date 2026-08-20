import { createFileRoute } from "@tanstack/react-router"

import { EditCustomer } from "@/features/customers/edit"

export const Route = createFileRoute("/customers_/$id/edit")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditCustomer id={id} />
}
