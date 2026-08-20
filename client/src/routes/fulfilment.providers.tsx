import { createFileRoute } from "@tanstack/react-router"

import { FulfilmentProviders } from "@/features/fulfilment/providers"

export const Route = createFileRoute("/fulfilment/providers")({
  component: FulfilmentProviders,
})
