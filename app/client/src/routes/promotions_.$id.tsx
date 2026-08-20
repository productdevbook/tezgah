import { createFileRoute } from "@tanstack/react-router"

import { PromotionDetail } from "@/features/promotions/detail"

export const Route = createFileRoute("/promotions_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <PromotionDetail id={id} />
}
