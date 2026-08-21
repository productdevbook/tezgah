import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { PriceLists } from "@/features/pricing/price-lists"

const priceListsSearch = z.object({
  after: z.string().optional(),
  q: z.string().optional(),
  status: z.enum(["active", "draft"]).optional(),
})

export const Route = createFileRoute("/pricing/price-lists")({
  validateSearch: priceListsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, q, status } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <PriceLists
      after={after}
      q={q}
      status={status ?? "all"}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
      onQChange={(next) =>
        // The cursor goes with it: it names a row in the ordering it was
        // issued under and means nothing under another filter.
        void navigate({
          search: (prev) => ({ ...prev, q: next, after: undefined }),
        })
      }
      onStatusChange={(next) =>
        void navigate({
          search: (prev) => ({
            ...prev,
            status: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
    />
  )
}
