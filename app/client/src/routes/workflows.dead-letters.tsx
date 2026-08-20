import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { DeadLetters } from "@/features/workflows/dead-letters"

const deadLettersSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/workflows/dead-letters")({
  validateSearch: deadLettersSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <DeadLetters
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
    />
  )
}
