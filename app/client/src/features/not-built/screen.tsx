import {
  Empty,
  EmptyDescription,
  EmptyHeader,
  EmptyTitle,
} from "@/components/ui/empty"
import type { Section } from "@/lib/nav"
import { PageHeading } from "@/components/page-heading"

/**
 * A section the API answers for and this panel does not draw at this
 * address. `section.folded` is checked ahead of `built`: a folded domain can
 * be fully drawn (`digital` is) and still land here on a direct visit to its
 * slug, since its screens live on the records they hang off rather than at
 * an address of their own — saying "no screen yet" then would be exactly the
 * lie this panel exists to avoid.
 */
export function NotBuilt({ section }: { section: Section }) {
  return (
    <div className="space-y-4">
      <PageHeading title={section.title} />
      <Empty>
        <EmptyHeader>
          <EmptyTitle>
            {section.folded ? "Not a place of its own" : "No screen yet"}
          </EmptyTitle>
          <EmptyDescription>
            {section.folded ? (
              <>
                This domain has no address here — its {section.operations}{" "}
                operations are drawn into {section.folded}.
              </>
            ) : (
              <>
                The admin API declares {section.operations} operations tagged{" "}
                <code>{section.tag}</code>. They work; nothing here draws them.
              </>
            )}
          </EmptyDescription>
        </EmptyHeader>
      </Empty>
    </div>
  )
}
