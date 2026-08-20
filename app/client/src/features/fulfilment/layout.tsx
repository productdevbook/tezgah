import { FulfilmentTabs } from "@/components/fulfilment-tabs"
import { PageHeading } from "@/components/page-heading"

/** Chrome shared by every `/fulfilment/*` tab — see `features/store/layout.tsx`. */
export function FulfilmentLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Fulfilment"
        subtitle="Who carries it, what it ships in, and what a shop charges to send it."
      />
      <FulfilmentTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
