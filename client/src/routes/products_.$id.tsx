import { createFileRoute } from "@tanstack/react-router"

import { ProductDetail } from "@/features/products/detail"

export const Route = createFileRoute("/products_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <ProductDetail id={id} />
}
