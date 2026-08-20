import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { PricePreferences } from "@/features/pricing/price-preferences"

const pricePreferencesSearch = z.object({
  attribute: z.string().optional(),
  value: z.string().optional(),
})

export const Route = createFileRoute("/pricing/price-preferences")({
  validateSearch: pricePreferencesSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { attribute, value } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <PricePreferences
      attribute={attribute}
      onAttributeChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, attribute: next }) })
      }
      value={value}
      onValueChange={(next) => void navigate({ search: (prev) => ({ ...prev, value: next }) })}
    />
  )
}
