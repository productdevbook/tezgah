import { createFileRoute } from "@tanstack/react-router"

import { TaxRegistrations } from "@/features/tax/registrations"

export const Route = createFileRoute("/tax/registrations")({
  component: TaxRegistrations,
})
