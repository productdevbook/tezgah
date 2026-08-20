import { Outlet, createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { StoreKeys } from "@/features/store/keys"

const keysSearch = z.object({ after: z.string().optional() })

export const Route = createFileRoute("/store/keys")({
  validateSearch: keysSearch,
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
      <StoreKeys
        after={after}
        onAfterChange={(next) =>
          void navigate({ search: (prev) => ({ ...prev, after: next }) })
        }
      />
      <Outlet />
    </>
  )
}
