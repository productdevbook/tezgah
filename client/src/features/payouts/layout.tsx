import { PayoutsTabs } from "@/components/payouts-tabs"
import { PageHeading } from "@/components/page-heading"

/**
 * Chrome shared by `/payouts` and `/payouts/commission-rules`, the same way
 * `features/store/layout.tsx` wraps `/store/*`. `payout-balance/{currency}`
 * and `orders/{id}/payout-lines` have no list of their own to be a third tab
 * — both are lookups by a key the operator supplies, so they live as
 * widgets on the payouts tab instead (`features/payouts/screen.tsx`).
 */
export function PayoutsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Payouts"
        subtitle="What a seller-scope is owed, and the commission that decides it."
      />
      <PayoutsTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
