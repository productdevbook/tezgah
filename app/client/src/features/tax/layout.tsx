import { TaxTabs } from "@/components/tax-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/** Chrome shared by every `/tax/*` tab — see `features/store/layout.tsx`. */
export function TaxLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading title={t("nav.tax")} subtitle={t("layout.tax.why")} />
      <TaxTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
