import { createFileRoute } from "@tanstack/react-router"

import { EditOrganisation } from "@/features/products/organisation"

export const Route = createFileRoute("/products_/$id/organisation")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditOrganisation id={id} />
}
