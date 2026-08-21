import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { priceList, type PriceList } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<PriceList> = [
  {
    header: "field.title",
    accessorKey: "title",
    meta: { className: "font-medium" },
  },
  {
    header: "field.kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
  },
  {
    header: "field.status",
    accessorKey: "status",
    cell: ({ row }) => <Badge>{row.original.status}</Badge>,
  },
  {
    header: "field.rules",
    accessorKey: "rules_count",
    meta: { className: "text-right font-mono text-xs" },
  },
]

export function PriceLists({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(["price-lists"], "/admin/price-lists", priceList, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: t("frame.priceLists"),
        description: t("frame.priceListsWhy"),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/pricing/price-lists/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.title}`}
        />
      )}
      empty={{
        title: t("empty.priceLists"),
        description: t("empty.priceListsWhy"),
      }}
    />
  )
}
