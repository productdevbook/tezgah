import { Outlet, createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { productStatus } from "@/api/schemas"
import { Products } from "@/features/products/screen"

const productsSearch = z.object({
  after: z.string().optional(),
  status: productStatus.optional(),
})

export const Route = createFileRoute("/products")({
  validateSearch: productsSearch,
  component: RouteComponent,
})

/**
 * The route owns the URL; the screen just takes props. `/products` — not
 * `/$section` — because a built section is a real page, and its cursor
 * belongs in that page's own address: `/products?status=draft&after=...` is
 * a link somebody else can open to the same page this one is on.
 *
 * The `<Outlet />` is what `/products/new` draws into: a creation form is a
 * child route rendered over this list rather than a page that replaces it,
 * so cancelling lands back on the page of products that was open, with the
 * cursor it had.
 */
export function RouteComponent() {
  const { status, after } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <>
      <Products
        status={status ?? "all"}
        after={after}
        onStatusChange={(next) =>
          void navigate({
            search: (prev) => ({
              ...prev,
              status: next === "all" ? undefined : next,
              after: undefined,
            }),
          })
        }
        onAfterChange={(next) =>
          void navigate({ search: (prev) => ({ ...prev, after: next }) })
        }
      />
      <Outlet />
    </>
  )
}
