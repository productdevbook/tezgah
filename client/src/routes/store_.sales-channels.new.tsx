import { createFileRoute } from "@tanstack/react-router"

import { NewSalesChannel } from "@/features/store/new-sales-channel"

export const Route = createFileRoute("/store_/sales-channels/new")({
  component: NewSalesChannel,
})
