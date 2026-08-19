import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import type { Section } from "@/lib/nav"
import { PageHeading } from "@/screens/page-heading"

/**
 * A section the API answers for and this panel does not draw yet.
 *
 * It names the number rather than saying "coming soon": the operations exist
 * and are reachable by anything that speaks HTTP, so what is missing is the
 * screen, not the feature.
 */
export function NotBuilt({ section }: { section: Section }) {
  return (
    <div className="space-y-4">
      <PageHeading title={section.title} />
      <Empty>
        <EmptyHeader>
          <EmptyTitle>No screen yet</EmptyTitle>
          <EmptyDescription>
            The admin API declares {section.operations} operations tagged{" "}
            <code>{section.tag}</code>. They work; nothing here draws them.
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    </div>
  )
}
