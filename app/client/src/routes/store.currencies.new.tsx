import { createFileRoute } from "@tanstack/react-router"

import { NewCurrency } from "@/features/store/new-currency"

/**
 * A child of the tab it creates into, not a sibling: the form is drawn in a
 * focus modal over the list, so the address stays linkable and closing it
 * leaves the operator where they were.
 */
export const Route = createFileRoute("/store/currencies/new")({
  component: NewCurrency,
})
