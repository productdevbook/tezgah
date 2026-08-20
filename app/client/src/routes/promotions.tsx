import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Promotions } from "@/features/promotions/screen"

const promotionsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/promotions")({
  validateSearch: promotionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Promotions
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
