import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { taxRegion, type TaxRegion } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<TaxRegion> = [
  {
    header: "field.country",
    accessorKey: "country_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "field.province",
    accessorKey: "province_code",
    cell: ({ row }) =>
      row.original.province_code ? (
        <span className="font-mono text-xs uppercase">
          {row.original.province_code}
        </span>
      ) : (
        <Empty />
      ),
  },
  {
    header: "field.provider",
    accessorKey: "provider",
    cell: ({ row }) => row.original.provider ?? <Empty />,
    meta: { className: "text-right" },
  },
]

export function TaxRegions({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(["tax-regions"], "/admin/tax-regions", taxRegion, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: t("frame.taxRegions"),
        description: t("frame.taxRegionsWhy"),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/tax/regions/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.country_code}`}
        />
      )}
      empty={{
        title: t("empty.taxRegions"),
        description: t("empty.taxRegionsWhy"),
      }}
    />
  )
}
