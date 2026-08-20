import { createFileRoute } from "@tanstack/react-router"

import { GiftCardDetail } from "@/features/credit/detail"

export const Route = createFileRoute("/credit_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <GiftCardDetail id={id} />
}
