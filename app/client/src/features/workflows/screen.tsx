import { Link } from "@tanstack/react-router"

import {
  workflowRunSummary,
  workflowRunState,
  type WorkflowRunSummary,
  type WorkflowRunState,
} from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/**
 * A run's own six states. `done` is the only unambiguous success; `reverted`
 * is a run that failed and finished unwinding what it started, not a run
 * that is still failing.
 */
function tone(
  state: WorkflowRunState
): "default" | "secondary" | "outline" | "destructive" {
  if (state === "done") return "default"
  if (state === "failed") return "destructive"
  if (state === "running" || state === "compensating") return "secondary"
  return "outline"
}

const columns: Columns<WorkflowRunSummary> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.transactionKey",
    accessorKey: "transaction_key",
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "field.state",
    accessorKey: "state",
    cell: ({ row }) => (
      <Badge variant={tone(row.original.state)}>{row.original.state}</Badge>
    ),
  },
  {
    header: "field.started",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Workflows({
  state,
  after,
  onStateChange,
  onAfterChange,
}: {
  state: WorkflowRunState | "all"
  after: string | undefined
  onStateChange: (state: WorkflowRunState | "all") => void
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["workflow-runs", state],
    "/admin/workflows-executions",
    workflowRunSummary,
    {
      after,
      onAfterChange,
      query: { state: state === "all" ? undefined : state },
    }
  )

  return (
    <div className="space-y-4">
      <PageHeading
        title={t("section.executions")}
        subtitle={t("section.executionsWhy")}
      >
        <Select
          value={state}
          onValueChange={(v) => onStateChange(v as WorkflowRunState | "all")}
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder="Any state" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Any state</SelectItem>
            {workflowRunState.options.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </PageHeading>

      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/workflows/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open run ${row.name}`}
          />
        )}
        empty={{
          title: "No runs",
          description:
            state === "all"
              ? "Nothing has run yet."
              : `No runs with state ${state}.`,
        }}
      />
    </div>
  )
}
