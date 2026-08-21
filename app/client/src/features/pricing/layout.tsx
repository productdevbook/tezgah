import { PricingTabs } from "@/components/pricing-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/** Chrome shared by every `/pricing/*` tab — see `features/store/layout.tsx`. */
export function PricingLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("nav.pricing")}
        subtitle={t("layout.pricing.why")}
      />
      <PricingTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
