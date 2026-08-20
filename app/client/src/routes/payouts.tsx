import { createFileRoute, Outlet } from "@tanstack/react-router"

import { PayoutsLayout } from "@/features/payouts/layout"

/** The layout every `/payouts/*` tab renders inside — see `routes/store.tsx`. */
export const Route = createFileRoute("/payouts")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <PayoutsLayout>
      <Outlet />
    </PayoutsLayout>
  )
}
