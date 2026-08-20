import { createFileRoute } from "@tanstack/react-router"

import { OrderDetail } from "@/features/orders/detail"

export const Route = createFileRoute("/orders_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <OrderDetail id={id} />
}
