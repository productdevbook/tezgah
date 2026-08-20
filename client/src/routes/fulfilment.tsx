import { createFileRoute, Outlet } from "@tanstack/react-router"

import { FulfilmentLayout } from "@/features/fulfilment/layout"

/** The layout every `/fulfilment/*` tab renders inside — see `routes/store.tsx`. */
export const Route = createFileRoute("/fulfilment")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <FulfilmentLayout>
      <Outlet />
    </FulfilmentLayout>
  )
}
