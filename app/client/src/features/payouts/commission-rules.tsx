import { commissionRule, type CommissionRule } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<CommissionRule> = [
  {
    header: "Scope",
    accessorKey: "category_id",
    cell: ({ row }) =>
      row.original.category_id ? (
        <span className="font-mono text-xs">{row.original.category_id}</span>
      ) : (
        <span className="text-xs text-muted-foreground">every category</span>
      ),
  },
  {
    header: "Kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
  },
  {
    header: "Value",
    accessorKey: "value",
    cell: ({ row }) =>
      row.original.kind === "percentage"
        ? `${row.original.value}%`
        : `${row.original.value} ${row.original.currency_code?.toUpperCase() ?? ""}`,
    meta: { className: "text-right font-mono text-xs" },
  },
]

export function CommissionRules({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["commission-rules"],
    "/admin/commission-rules",
    commissionRule,
    {
      after,
      onAfterChange,
    }
  )

  return (
    <DataTable
      header={{
        title: "Commission rules",
        description:
          "What the marketplace keeps from a seller's line, and on what.",
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: "No commission rules",
        description:
          "A category with no rule and no default earns no commission — nothing is taken until one is set.",
      }}
    />
  )
}
