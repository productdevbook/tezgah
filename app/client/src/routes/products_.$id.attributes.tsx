import { createFileRoute } from "@tanstack/react-router"

import { EditAttributes } from "@/features/products/attributes"

export const Route = createFileRoute("/products_/$id/attributes")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditAttributes id={id} />
}
