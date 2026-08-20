import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Customers } from "@/features/customers/screen"

const customersSearch = z.object({
  after: z.string().optional(),
  q: z.string().optional(),
})

export const Route = createFileRoute("/customers")({
  validateSearch: customersSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, q } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Customers
      after={after}
      q={q}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
      onQChange={(next) =>
        // The cursor goes with it: a cursor names a row in the ordering it was
        // issued under and means nothing under another filter.
        void navigate({
          search: (prev) => ({ ...prev, q: next, after: undefined }),
        })
      }
    />
  )
}
