import { createFileRoute } from "@tanstack/react-router"

import { EditProduct } from "@/features/products/edit"

export const Route = createFileRoute("/products_/$id/edit")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditProduct id={id} />
}
