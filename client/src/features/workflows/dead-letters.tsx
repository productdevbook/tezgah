import { Link } from "@tanstack/react-router"

import { workflowDeadLetter, type WorkflowDeadLetter } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { usePagedList } from "@/lib/paged"

/**
 * Scope-wide, with no per-letter owner (`src/api/admin_rest.rs`'s
 * `list_workflow_dead_letters`) — every row here belongs to whichever run
 * failed, not to one customer, so nothing narrows it further.
 */
const columns: Columns<WorkflowDeadLetter> = [
  { header: "Step", accessorKey: "step_name", meta: { className: "font-medium" } },
  { header: "Failure", accessorKey: "failure", meta: { className: "max-w-md truncate" } },
  {
    header: "Run",
    accessorKey: "run_id",
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "When",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function DeadLetters({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["workflow-dead-letters"],
    "/admin/workflow-dead-letters",
    workflowDeadLetter,
    { after, onAfterChange }
  )

  return (
    <DataTable
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/workflows/$id"
          params={{ id: row.run_id }}
          className="absolute inset-0"
          aria-label={`Open the run behind ${row.step_name}`}
        />
      )}
      empty={{
        title: "No dead letters",
        description: "Nothing has run out of retries and been given up on.",
      }}
    />
  )
}
