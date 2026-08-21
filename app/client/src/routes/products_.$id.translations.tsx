import { createFileRoute } from "@tanstack/react-router"

import { EditTranslations } from "@/features/products/translations"

export const Route = createFileRoute("/products_/$id/translations")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditTranslations id={id} />
}
