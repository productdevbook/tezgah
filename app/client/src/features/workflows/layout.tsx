import { WorkflowsTabs } from "@/components/workflows-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/** Chrome shared by `/workflows` and `/workflows/dead-letters`. */
export function WorkflowsLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("nav.workflows")}
        subtitle={t("layout.workflows.why")}
      />
      <WorkflowsTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
