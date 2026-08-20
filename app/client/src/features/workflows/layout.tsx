import { WorkflowsTabs } from "@/components/workflows-tabs"
import { PageHeading } from "@/components/page-heading"

/** Chrome shared by `/workflows` and `/workflows/dead-letters`. */
export function WorkflowsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Workflows"
        subtitle="Every run the runner has driven, and every step it could not finish."
      />
      <WorkflowsTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
