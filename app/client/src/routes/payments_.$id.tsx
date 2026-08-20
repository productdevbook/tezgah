import { createFileRoute } from "@tanstack/react-router"

import { PaymentDetail } from "@/features/payments/detail"

/** A full page, not a `/payments` tab's content — same reasoning as `store_.regions.$id.tsx`. */
export const Route = createFileRoute("/payments_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <PaymentDetail id={id} />
}
