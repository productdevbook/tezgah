import { Link } from "@tanstack/react-router"

import { subscription, type Subscription } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

const columns: Columns<Subscription> = [
  {
    header: "field.status",
    accessorKey: "status",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant={row.original.ended_at ? "outline" : "default"}>
          {row.original.status}
        </Badge>
        {row.original.cancel_at_period_end ? (
          <Badge variant="outline">ends this period</Badge>
        ) : null}
      </div>
    ),
  },
  {
    header: "field.cycle",
    accessorKey: "cycle",
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.nextCharge",
    accessorKey: "next_billing_at",
    cell: ({ row }) =>
      row.original.ended_at ? (
        <span className="text-muted-foreground">ended</span>
      ) : (
        new Date(row.original.next_billing_at).toLocaleDateString()
      ),
  },
  {
    header: "field.currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "field.dunning",
    accessorKey: "dunning_attempts",
    /* Above zero means a charge failed and is being retried, which is a
       different thing from a cancelled contract. */
    cell: ({ row }) =>
      row.original.dunning_attempts > 0 ? (
        <Badge variant="destructive">{row.original.dunning_attempts}</Badge>
      ) : (
        <span className="text-xs text-muted-foreground">—</span>
      ),
    meta: { className: "text-right" },
  },
]

export function Subscriptions({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["subscriptions"],
    "/admin/subscriptions",
    subscription,
    {
      after,
      onAfterChange,
    }
  )
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.subscriptions.title")}
        subtitle={t("screen.subscriptions.subtitle")}
      />
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/subscriptions/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open subscription ${row.id}`}
          />
        )}
        empty={{
          title: t("screen.subscriptions.empty"),
          description: t("screen.subscriptions.emptyAny"),
        }}
      />
    </div>
  )
}
