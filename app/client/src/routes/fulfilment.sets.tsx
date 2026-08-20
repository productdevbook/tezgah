import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { FulfilmentSets } from "@/features/fulfilment/sets"

const setsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/fulfilment/sets")({
  validateSearch: setsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <FulfilmentSets
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
