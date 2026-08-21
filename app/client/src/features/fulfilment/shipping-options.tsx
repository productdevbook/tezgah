import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { shippingOption, type ShippingOption } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<ShippingOption> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.priceType",
    accessorKey: "price_type",
    cell: ({ row }) => (
      <Badge variant="outline">{row.original.price_type}</Badge>
    ),
  },
  {
    header: "field.return",
    accessorKey: "is_return",
    cell: ({ row }) => (row.original.is_return ? "return" : "outbound"),
  },
  {
    header: "field.inStore",
    accessorKey: "enabled_in_store",
    cell: ({ row }) => (row.original.enabled_in_store ? "enabled" : "disabled"),
    meta: { className: "text-right" },
  },
]

export function ShippingOptions({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["shipping-options"],
    "/admin/shipping-options",
    shippingOption,
    {
      after,
      onAfterChange,
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.shippingOptions"),
        description: t("frame.shippingOptionsWhy"),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/fulfilment/shipping-options/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{
        title: t("empty.shippingOptions"),
        description: t("empty.shippingOptionsWhy"),
      }}
    />
  )
}
