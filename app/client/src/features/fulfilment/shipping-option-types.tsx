import { shippingOptionType, type ShippingOptionType } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<ShippingOptionType> = [
  {
    header: "Code",
    accessorKey: "code",
    meta: { className: "font-mono text-xs" },
  },
  { header: "Label", accessorKey: "label", meta: { className: "font-medium" } },
  {
    header: "Description",
    accessorKey: "description",
    cell: ({ row }) => row.original.description ?? <Empty />,
    meta: { className: "max-w-96 truncate" },
  },
]

/** No `GET .../{id}` — only `POST` adds one — so a row here goes nowhere. */
export function ShippingOptionTypes({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["shipping-option-types"],
    "/admin/shipping-option-types",
    shippingOptionType,
    { after, onAfterChange }
  )

  return (
    <DataTable
      header={{
        title: "Option types",
        description:
          "The labels a shopper picks between — standard, express — shared across options.",
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: "No shipping option types",
        description:
          'A type is a label a shipping option can carry, like "express".',
      }}
    />
  )
}
