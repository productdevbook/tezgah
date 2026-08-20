import { createFileRoute } from "@tanstack/react-router"

import { NewCurrency } from "@/features/store/new-currency"

export const Route = createFileRoute("/store_/currencies/new")({
  component: NewCurrency,
})
