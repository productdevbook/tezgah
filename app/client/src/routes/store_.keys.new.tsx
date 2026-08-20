import { createFileRoute } from "@tanstack/react-router"

import { NewPublishableKey } from "@/features/store/new-key"

export const Route = createFileRoute("/store_/keys/new")({
  component: NewPublishableKey,
})
