import { PaymentsTabs } from "@/components/payments-tabs"
import { PageHeading } from "@/components/page-heading"

/** Chrome shared by `/payments` and `/payments/refund-reasons` — see `features/payouts/layout.tsx`. */
export function PaymentsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Payments"
        subtitle="What was taken against an order, and why a refund might be given."
      />
      <PaymentsTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
