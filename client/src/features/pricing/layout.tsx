import { PricingTabs } from "@/components/pricing-tabs"
import { PageHeading } from "@/components/page-heading"

/** Chrome shared by every `/pricing/*` tab — see `features/store/layout.tsx`. */
export function PricingLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading title="Pricing" subtitle="Lists, one preference, and the sets and rows behind a price." />
      <PricingTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
