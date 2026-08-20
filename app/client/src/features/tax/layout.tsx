import { TaxTabs } from "@/components/tax-tabs"
import { PageHeading } from "@/components/page-heading"

/** Chrome shared by every `/tax/*` tab — see `features/store/layout.tsx`. */
export function TaxLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Tax"
        subtitle="What is charged where, and what the shop itself is registered under."
      />
      <TaxTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
