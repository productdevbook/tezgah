import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { ShippingOptions } from "@/features/fulfilment/shipping-options"

const shippingOptionsSearch = z.object({
  after: z.string().optional(),
  q: z.string().optional(),
  kind: z.enum(["outbound", "return"]).optional(),
})

export const Route = createFileRoute("/fulfilment/shipping-options")({
  validateSearch: shippingOptionsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, q, kind } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <ShippingOptions
      after={after}
      q={q}
      kind={kind ?? "all"}
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
      onKindChange={(next) =>
        void navigate({
          search: (prev) => ({
            ...prev,
            kind: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
    />
  )
}
