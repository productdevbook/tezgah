import { createFileRoute } from "@tanstack/react-router"

import { EditPromotion } from "@/features/promotions/edit"

export const Route = createFileRoute("/promotions_/$id/edit")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <EditPromotion id={id} />
}
