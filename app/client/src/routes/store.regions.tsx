import { Outlet, createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { StoreRegions } from "@/features/store/regions"

const regionsSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/store/regions")({
  validateSearch: regionsSearch,
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what this tab's `new` route draws into — a creation
 * form is a focus modal over the list, not a page that replaces it.
 */
export function RouteComponent() {
  const { after } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <>
      <StoreRegions
        after={after}
        onAfterChange={(next) =>
          void navigate({ search: (prev) => ({ ...prev, after: next }) })
        }
      />
      <Outlet />
    </>
  )
}
