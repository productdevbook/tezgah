import { useRouter } from "@tanstack/react-router"

import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"

/**
 * `router.history.back()`, not a `Link` to the list's own address: the list
 * keeps its cursor in the URL (`lib/paged.ts`), and going back in history is
 * what lands on the exact page a row was clicked from — a link to, say,
 * `/products` on its own would always reopen page one.
 */
export function DetailHeader({
  back,
  title,
  subtitle,
  children,
}: {
  back: string
  title: string
  subtitle?: string
  children?: React.ReactNode
}) {
  const router = useRouter()
  return (
    <div className="space-y-2">
      <Button
        variant="ghost"
        size="sm"
        className="-ml-2 text-muted-foreground"
        onClick={() => router.history.back()}
      >
        ← Back to {back}
      </Button>
      <PageHeading title={title} subtitle={subtitle}>
        {children}
      </PageHeading>
    </div>
  )
}
