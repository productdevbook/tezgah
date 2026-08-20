import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { BasketDetail } from "@/features/baskets/detail"

const basketSearch = z.object({
  cartsAfter: z.string().optional(),
  ordersAfter: z.string().optional(),
})

export const Route = createFileRoute("/baskets_/$id")({
  validateSearch: basketSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  const { cartsAfter, ordersAfter } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <BasketDetail
      id={id}
      cartsAfter={cartsAfter}
      ordersAfter={ordersAfter}
      onCartsAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, cartsAfter: next }) })
      }
      onOrdersAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, ordersAfter: next }) })
      }
    />
  )
}
