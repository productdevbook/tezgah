import { createFileRoute } from "@tanstack/react-router"

import { Records } from "@/features/records/screen"

export const Route = createFileRoute("/records")({
  component: Records,
})
