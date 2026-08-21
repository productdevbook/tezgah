import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { taxRate, type TaxRate } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<TaxRate> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.code",
    accessorKey: "code",
    cell: ({ row }) => row.original.code ?? <Empty />,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.rate",
    accessorKey: "rate",
    meta: { className: "text-right font-mono text-xs" },
  },
  {
    header: "field.combinable",
    accessorKey: "is_combinable",
    cell: ({ row }) =>
      row.original.is_combinable ? (
        <Badge variant="outline">combinable</Badge>
      ) : null,
  },
  {
    header: "field.default",
    accessorKey: "is_default",
    cell: ({ row }) =>
      row.original.is_default ? <Badge>default</Badge> : null,
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
  const t = useT()
  const paged = usePagedList(["tax-rates"], "/admin/tax-rates", taxRate, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: t("frame.taxRates"),
        description: t("frame.taxRatesWhy"),
      }}
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
      empty={{
        title: t("empty.taxRates"),
        description: t("empty.taxRatesWhy"),
      }}
    />
  )
}
