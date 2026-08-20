import { Link } from "@tanstack/react-router"

import { shippingOption, type ShippingOption } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<ShippingOption> = [
  { header: "Name", accessorKey: "name", meta: { className: "font-medium" } },
  {
    header: "Price type",
    accessorKey: "price_type",
    cell: ({ row }) => (
      <Badge variant="outline">{row.original.price_type}</Badge>
    ),
  },
  {
    header: "Return",
    accessorKey: "is_return",
    cell: ({ row }) => (row.original.is_return ? "return" : "outbound"),
  },
  {
    header: "In store",
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
        title: "Shipping options",
        description:
          "What a shopper can choose at the till, and what each costs.",
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
        title: "No shipping options",
        description:
          "A service zone offers nothing to ship with until one is added.",
      }}
    />
  )
}
