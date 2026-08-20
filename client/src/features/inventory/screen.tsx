import { inventoryItem, type InventoryItem } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"

const columns: Columns<InventoryItem> = [
  {
    header: "SKU",
    accessorKey: "sku",
    cell: ({ row }) =>
      row.original.sku ?? <span className="text-muted-foreground">none</span>,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Title",
    accessorKey: "title",
    cell: ({ row }) =>
      row.original.title ?? <span className="text-muted-foreground">untitled</span>,
    meta: { className: "max-w-96 truncate" },
  },
  {
    header: "Ships",
    accessorKey: "requires_shipping",
    cell: ({ row }) => (
      <Badge variant={row.original.requires_shipping ? "default" : "outline"}>
        {row.original.requires_shipping ? "shipped" : "digital"}
      </Badge>
    ),
    meta: { className: "text-right" },
  },
]

export function Inventory({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["inventory-items"],
    "/admin/inventory-items",
    inventoryItem,
    { after, onAfterChange },
  )
  return (
    <div className="space-y-4">
      <PageHeading
        title="Inventory"
        subtitle="An item is the thing counted. What is on hand is counted per location, one level down."
      />
      <DataTable
        paged={paged}
        columns={columns}
        empty={{ title: "Nothing stocked", description: "No inventory item exists yet." }}
      />
    </div>
  )
}
