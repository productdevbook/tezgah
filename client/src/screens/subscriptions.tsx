import { subscription, type Subscription } from "@/api/views"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/screens/page-heading"

const columns: Columns<Subscription> = [
  {
    header: "Status",
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
    header: "Cycle",
    accessorKey: "cycle",
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Next charge",
    accessorKey: "next_billing_at",
    cell: ({ row }) =>
      row.original.ended_at ? (
        <span className="text-muted-foreground">ended</span>
      ) : (
        new Date(row.original.next_billing_at).toLocaleDateString()
      ),
  },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "Dunning",
    accessorKey: "dunning_attempts",
    /* Above zero means a charge failed and is being retried, which is a
       different thing from a cancelled contract. */
    cell: ({ row }) =>
      row.original.dunning_attempts > 0 ? (
        <Badge variant="destructive">{row.original.dunning_attempts}</Badge>
      ) : (
        <span className="text-muted-foreground text-xs">—</span>
      ),
    meta: { className: "text-right" },
  },
]

export function Subscriptions() {
  const paged = usePagedList(["subscriptions"], "/admin/subscriptions", subscription)
  return (
    <div className="space-y-4">
      <PageHeading
        title="Subscriptions"
        subtitle="A contract, not an order. The orders it produces are listed under Orders."
      />
      <DataTable
        paged={paged}
        columns={columns}
        empty={{ title: "No subscriptions", description: "Nothing recurring is sold." }}
      />
    </div>
  )
}
