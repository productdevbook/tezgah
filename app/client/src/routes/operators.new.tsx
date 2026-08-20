import { createFileRoute } from "@tanstack/react-router"

import { NewOperatorForm } from "@/features/operators/new"

export const Route = createFileRoute("/operators/new")({
  component: NewOperatorForm,
})
