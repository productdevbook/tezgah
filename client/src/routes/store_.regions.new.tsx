import { createFileRoute } from "@tanstack/react-router"

import { NewRegion } from "@/features/store/new-region"

export const Route = createFileRoute("/store_/regions/new")({
  component: NewRegion,
})
