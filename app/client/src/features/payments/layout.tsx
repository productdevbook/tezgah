import { PaymentsTabs } from "@/components/payments-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/** Chrome shared by `/payments` and `/payments/refund-reasons` — see `features/payouts/layout.tsx`. */
export function PaymentsLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("nav.payments")}
        subtitle={t("layout.payments.why")}
      />
      <PaymentsTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
