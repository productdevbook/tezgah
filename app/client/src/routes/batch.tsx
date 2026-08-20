import { createFileRoute } from "@tanstack/react-router"

import { Batch } from "@/features/batch/screen"

export const Route = createFileRoute("/batch")({
  component: Batch,
})
