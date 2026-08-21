import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Payments } from "@/features/payments/payments"

const paymentsSearch = z.object({
  after: z.string().optional(),
  state: z.enum(["authorized", "captured", "canceled"]).optional(),
})

export const Route = createFileRoute("/payments/")({
  validateSearch: paymentsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, state } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Payments
      after={after}
      state={state ?? "all"}
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
      onStateChange={(next) =>
        // The cursor goes with it: it names a row in the ordering it was
        // issued under and means nothing under another filter.
        void navigate({
          search: (prev) => ({
            ...prev,
            state: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
    />
  )
}
