import { createFileRoute } from "@tanstack/react-router"

import { BasketSearch } from "@/features/baskets/search"

export const Route = createFileRoute("/baskets")({ component: RouteComponent })

export function RouteComponent() {
  const navigate = Route.useNavigate()
  return <BasketSearch onSearch={(id) => void navigate({ to: "/baskets/$id", params: { id } })} />
}
