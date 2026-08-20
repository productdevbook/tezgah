import { createFileRoute, Outlet } from "@tanstack/react-router"

import { PaymentsLayout } from "@/features/payments/layout"

/** The layout `/payments` and `/payments/refund-reasons` render inside — see `routes/payouts.tsx`. */
export const Route = createFileRoute("/payments")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <PaymentsLayout>
      <Outlet />
    </PaymentsLayout>
  )
}
