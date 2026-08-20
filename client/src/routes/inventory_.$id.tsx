import { createFileRoute } from "@tanstack/react-router"

import { InventoryItemDetail } from "@/features/inventory/detail"

export const Route = createFileRoute("/inventory_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <InventoryItemDetail id={id} />
}
