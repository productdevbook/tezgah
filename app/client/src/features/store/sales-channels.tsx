import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { salesChannel, type SalesChannel } from "@/api/schemas"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { DataTable, type Columns } from "@/components/data-table"
import { usePagedList } from "@/lib/paged"

const columns: Columns<SalesChannel> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.description",
    accessorKey: "description",
    cell: ({ row }) =>
      row.original.description ?? (
        <span className="text-muted-foreground">—</span>
      ),
    meta: { className: "max-w-96 truncate text-sm" },
  },
  {
    header: "field.state",
    accessorKey: "is_disabled",
    cell: ({ row }) => (
      <Badge variant={row.original.is_disabled ? "outline" : "default"}>
        {row.original.is_disabled ? "disabled" : "selling"}
      </Badge>
    ),
    meta: { className: "text-right" },
  },
]

export function StoreSalesChannels({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["sales-channels"],
    "/admin/sales-channels",
    salesChannel,
    { after, onAfterChange }
  )

  return (
    <DataTable
      header={{
        title: t("frame.salesChannels"),
        description: t("frame.salesChannelsWhy"),
        actions: (
          <Button
            size="sm"
            nativeButton={false}
            render={<Link to="/store/sales-channels/new" />}
          >
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
            New sales channel
          </Button>
        ),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/store/sales-channels/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{
        title: t("empty.salesChannels"),
        description: t("empty.salesChannelsWhy"),
      }}
    />
  )
}
