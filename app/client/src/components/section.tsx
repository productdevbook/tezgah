import type { ReactNode } from "react"

import { cn } from "@/lib/utils"

/**
 * A record's page is a stack of these, not one long form.
 *
 * Each section owns one part of the record, says what it holds, and carries
 * its own actions — so editing a product's media and editing its options are
 * two small forms with two addresses rather than one screen that saves
 * everything at once and cannot say what changed.
 */
export function Section({
  title,
  description,
  actions,
  children,
  className,
}: {
  title: string
  description?: string
  actions?: ReactNode
  children?: ReactNode
  className?: string
}) {
  return (
    <section
      className={cn("overflow-hidden rounded-xl border bg-card", className)}
    >
      <header className="flex items-start justify-between gap-4 px-6 py-4">
        <div className="min-w-0">
          <h2 className="truncate text-base font-medium">{title}</h2>
          {description ? (
            <p className="mt-0.5 text-sm text-muted-foreground">
              {description}
            </p>
          ) : null}
        </div>
        {actions ? (
          <div className="flex shrink-0 items-center gap-2">{actions}</div>
        ) : null}
      </header>
      {children ? <div className="border-t">{children}</div> : null}
    </section>
  )
}

/**
 * One fact inside a section: a label, what it says, and — where the fact is
 * its own thing to change — an action beside it.
 */
export function SectionRow({
  label,
  value,
  action,
}: {
  label: string
  value?: ReactNode
  action?: ReactNode
}) {
  const plain = value === null || value === undefined || value === ""

  return (
    <div
      className={cn(
        "grid w-full items-center gap-4 px-6 py-3 text-muted-foreground",
        action ? "grid-cols-[1fr_1fr_auto]" : "grid-cols-2"
      )}
    >
      <span className="truncate text-sm font-medium text-foreground">
        {label}
      </span>
      {plain ? (
        <span className="text-sm">—</span>
      ) : (
        <div className="min-w-0 text-sm break-words text-foreground">
          {value}
        </div>
      )}
      {action ? <div className="justify-self-end">{action}</div> : null}
    </div>
  )
}

/** Rows, separated. Used inside `Section` where the body is a list of facts. */
export function SectionRows({ children }: { children: ReactNode }) {
  return <div className="divide-y">{children}</div>
}
