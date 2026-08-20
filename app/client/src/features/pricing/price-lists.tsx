import { Link } from "@tanstack/react-router"

import { priceList, type PriceList } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<PriceList> = [
  { header: "Title", accessorKey: "title", meta: { className: "font-medium" } },
  {
    header: "Kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
  },
  {
    header: "Status",
    accessorKey: "status",
    cell: ({ row }) => <Badge>{row.original.status}</Badge>,
  },
  {
    header: "Rules",
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
  const paged = usePagedList(["price-lists"], "/admin/price-lists", priceList, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
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
        title: "No price lists",
        description: "A price list overrides a price set's own prices for a rule it matches.",
      }}
    />
  )
}
