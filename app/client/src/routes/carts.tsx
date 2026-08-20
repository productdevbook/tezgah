import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Carts } from "@/features/carts/screen"

const cartsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/carts")({
  validateSearch: cartsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Carts
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
