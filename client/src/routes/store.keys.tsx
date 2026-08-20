import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { StoreKeys } from "@/features/store/keys"

const keysSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/store/keys")({
  validateSearch: keysSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <StoreKeys
      after={after}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
