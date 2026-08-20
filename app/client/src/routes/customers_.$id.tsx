import { createFileRoute } from "@tanstack/react-router"

import { CustomerDetail } from "@/features/customers/detail"

export const Route = createFileRoute("/customers_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <CustomerDetail id={id} />
}
