import { createFileRoute, Outlet } from "@tanstack/react-router"

import { PricingLayout } from "@/features/pricing/layout"

/** The layout every `/pricing/*` tab renders inside — see `routes/store.tsx`. */
export const Route = createFileRoute("/pricing")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <PricingLayout>
      <Outlet />
    </PricingLayout>
  )
}
