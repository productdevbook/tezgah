import { createFileRoute, Outlet } from "@tanstack/react-router"

import { TaxLayout } from "@/features/tax/layout"

/** The layout every `/tax/*` tab renders inside — see `routes/store.tsx`. */
export const Route = createFileRoute("/tax")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <TaxLayout>
      <Outlet />
    </TaxLayout>
  )
}
