import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { GiftCards } from "@/features/credit/screen"

const creditSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/credit")({
  validateSearch: creditSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <GiftCards
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
