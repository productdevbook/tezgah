import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { ProductDetail } from "@/features/products/detail"

const productSearch = z.object({
  variant: z.string().optional(),
})

export const Route = createFileRoute("/products_/$id")({
  validateSearch: productSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  const { variant } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <ProductDetail
      id={id}
      variantId={variant}
      onVariantIdChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, variant: next }) })
      }
    />
  )
}
