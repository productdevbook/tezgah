import { Link } from "@tanstack/react-router"

import { taxRate, type TaxRate } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<TaxRate> = [
  { header: "Name", accessorKey: "name", meta: { className: "font-medium" } },
  {
    header: "Code",
    accessorKey: "code",
    cell: ({ row }) => row.original.code ?? <Empty />,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Rate",
    accessorKey: "rate",
    meta: { className: "text-right font-mono text-xs" },
  },
  {
    header: "Combinable",
    accessorKey: "is_combinable",
    cell: ({ row }) => (row.original.is_combinable ? <Badge variant="outline">combinable</Badge> : null),
  },
  {
    header: "Default",
    accessorKey: "is_default",
    cell: ({ row }) => (row.original.is_default ? <Badge>default</Badge> : null),
    meta: { className: "text-right" },
  },
]

export function TaxRates({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["tax-rates"], "/admin/tax-rates", taxRate, { after, onAfterChange })

  return (
    <DataTable
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/tax/rates/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{ title: "No tax rates", description: "A region charges nothing until a rate is set." }}
    />
  )
}
