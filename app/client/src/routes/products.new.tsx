import { createFileRoute } from "@tanstack/react-router"

import { CreateProduct } from "@/features/products/create"

/**
 * A child of `/products`, not a sibling: the creation form is drawn over the
 * list in a focus modal, so the address is still linkable and the page
 * underneath is still there when it closes.
 */
export const Route = createFileRoute("/products/new")({
  component: CreateProduct,
})
