import { promotion, type Promotion } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"

const columns: Columns<Promotion> = [
  {
    header: "Code",
    accessorKey: "code",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <span className="font-mono text-xs">{row.original.code}</span>
        {row.original.is_automatic ? (
          <Badge variant="outline">automatic</Badge>
        ) : null}
      </div>
    ),
  },
  { header: "Kind", accessorKey: "kind", meta: { className: "text-sm" } },
  {
    header: "Status",
    accessorKey: "status",
    cell: ({ row }) => (
      <Badge variant={row.original.status === "active" ? "default" : "outline"}>
        {row.original.status}
      </Badge>
    ),
  },
  {
    header: "Used",
    accessorKey: "used",
    // Claimed at checkout, not counted at payment: this is what is spoken for.
    cell: ({ row }) =>
      row.original.usage_limit === null
        ? `${row.original.used}`
        : `${row.original.used} / ${row.original.usage_limit}`,
    meta: { className: "text-right font-mono text-xs" },
  },
]

export function Promotions({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["promotions"], "/admin/promotions", promotion, {
    after,
    onAfterChange,
  })
  return (
    <div className="space-y-4">
      <PageHeading
        title="Promotions"
        subtitle="A use is claimed when a cart is checked out, not when it is paid for."
      />
      <DataTable
        paged={paged}
        columns={columns}
        empty={{ title: "No promotions", description: "Nothing is on offer." }}
      />
    </div>
  )
}
