import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Payouts } from "@/features/payouts/screen"

const payoutsSearch = z.object({
  after: z.string().optional(),
  currency: z.string().optional(),
  order: z.string().optional(),
  linesAfter: z.string().optional(),
})

export const Route = createFileRoute("/payouts/")({
  validateSearch: payoutsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { after, currency, order, linesAfter } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <Payouts
      after={after}
      onAfterChange={(next) => void navigate({ search: (prev) => ({ ...prev, after: next }) })}
      currency={currency}
      onCurrencyChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, currency: next }) })
      }
      orderId={order}
      onOrderIdChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, order: next, linesAfter: undefined }) })
      }
      linesAfter={linesAfter}
      onLinesAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, linesAfter: next }) })
      }
    />
  )
}
