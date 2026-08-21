import { commissionRule, type CommissionRule } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { useT } from "@/panel/i18n"

const columns: Columns<CommissionRule> = [
  {
    header: "field.scope",
    accessorKey: "category_id",
    cell: ({ row }) =>
      row.original.category_id ? (
        <span className="font-mono text-xs">{row.original.category_id}</span>
      ) : (
        <span className="text-xs text-muted-foreground">every category</span>
      ),
  },
  {
    header: "field.kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
  },
  {
    header: "field.value",
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
  const t = useT()
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
        title: t("frame.commissionRules"),
        description: t("frame.commissionRulesWhy"),
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: t("empty.commissionRules"),
        description: t("empty.commissionRulesWhy"),
      }}
    />
  )
}
