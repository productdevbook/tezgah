import { createFileRoute } from "@tanstack/react-router"

import { StoreKeys } from "@/features/store/keys"

export const Route = createFileRoute("/store/keys")({
  component: StoreKeys,
})
