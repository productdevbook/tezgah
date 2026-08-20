import { createFileRoute } from "@tanstack/react-router"

import { SubscriptionDetail } from "@/features/subscriptions/detail"

export const Route = createFileRoute("/subscriptions_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <SubscriptionDetail id={id} />
}
