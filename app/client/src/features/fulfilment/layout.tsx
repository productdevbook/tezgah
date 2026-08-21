import { FulfilmentTabs } from "@/components/fulfilment-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/** Chrome shared by every `/fulfilment/*` tab — see `features/store/layout.tsx`. */
export function FulfilmentLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("nav.fulfilment")}
        subtitle={t("layout.fulfilment.why")}
      />
      <FulfilmentTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
