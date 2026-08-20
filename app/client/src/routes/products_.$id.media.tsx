import { createFileRoute } from "@tanstack/react-router"

import { EditMedia } from "@/features/products/media"

export const Route = createFileRoute("/products_/$id/media")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditMedia id={id} />
}
