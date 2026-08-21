import { fulfilmentSet, type FulfilmentSet } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<FulfilmentSet> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
    meta: { className: "text-right" },
  },
]

/**
 * `DELETE /admin/fulfillment-sets/{id}` exists but `GET .../{id}` does not —
 * only the set's own service zones are fetched by id — so a row here goes
 * nowhere.
 */
export function FulfilmentSets({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["fulfilment-sets"],
    "/admin/fulfillment-sets",
    fulfilmentSet,
    {
      after,
      onAfterChange,
    }
  )

  return (
    <DataTable
      header={{
        title: "Fulfilment sets",
        description: "A set groups the service zones one carrier serves.",
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: "No fulfilment sets",
        description:
          "A set groups the service zones a location or a store ships through.",
      }}
    />
  )
}
