import { createFileRoute } from "@tanstack/react-router"

import { StoreCurrencies } from "@/features/store/currencies"

export const Route = createFileRoute("/store/currencies")({
  component: StoreCurrencies,
})
