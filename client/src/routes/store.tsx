import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { Store } from "@/features/store/screen"

const storeSearch = z.object({
  regionsAfter: z.string().optional(),
  channelsAfter: z.string().optional(),
})

export const Route = createFileRoute("/store")({
  validateSearch: storeSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { regionsAfter, channelsAfter } = Route.useSearch()
  const navigate = Route.useNavigate()
  return (
    <Store
      regionsAfter={regionsAfter}
      onRegionsAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, regionsAfter: next }) })
      }
      channelsAfter={channelsAfter}
      onChannelsAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, channelsAfter: next }) })
      }
    />
  )
}
