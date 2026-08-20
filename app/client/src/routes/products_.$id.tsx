import { Outlet, createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { ProductDetail } from "@/features/products/detail"

const productSearch = z.object({
  variant: z.string().optional(),
})

export const Route = createFileRoute("/products_/$id")({
  validateSearch: productSearch,
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what `/products/$id/edit` draws into. Without it that
 * address resolved to this page and the edit form was never rendered at all —
 * a child route whose parent draws no outlet is a screen nothing can reach.
 */
export function RouteComponent() {
  const { id } = Route.useParams()
  const { variant } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <>
      <ProductDetail
        id={id}
        variantId={variant}
        onVariantIdChange={(next) =>
          void navigate({ search: (prev) => ({ ...prev, variant: next }) })
        }
      />
      <Outlet />
    </>
  )
}
