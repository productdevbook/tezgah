import { Link } from "@tanstack/react-router"

import { inventoryItem, type InventoryItem } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

const columns: Columns<InventoryItem> = [
  {
    header: "field.sku",
    accessorKey: "sku",
    cell: ({ row }) =>
      row.original.sku ?? <span className="text-muted-foreground">none</span>,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.title",
    accessorKey: "title",
    cell: ({ row }) =>
      row.original.title ?? (
        <span className="text-muted-foreground">untitled</span>
      ),
    meta: { className: "max-w-96 truncate" },
  },
  {
    header: "field.ships",
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
  const t = useT()
  const paged = usePagedList(
    ["inventory-items"],
    "/admin/inventory-items",
    inventoryItem,
    { after, onAfterChange }
  )
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.inventory.title")}
        subtitle={t("screen.inventory.subtitle")}
      />
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/inventory/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open ${row.title ?? row.sku ?? "inventory item"}`}
          />
        )}
        empty={{
          title: t("screen.inventory.empty"),
          description: t("screen.inventory.emptyAny"),
        }}
      />
    </div>
  )
}
