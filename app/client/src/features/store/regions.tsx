import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { region, type Region } from "@/api/schemas"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { DataTable, type Columns } from "@/components/data-table"
import { usePagedList } from "@/lib/paged"

const columns: Columns<Region> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "field.tax",
    accessorKey: "is_tax_inclusive",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant="outline">
          {row.original.is_tax_inclusive ? "inclusive" : "exclusive"}
        </Badge>
        {row.original.has_automatic_taxes ? <Badge>automatic</Badge> : null}
      </div>
    ),
  },
  {
    header: "field.providers",
    accessorKey: "payment_providers",
    cell: ({ row }) =>
      row.original.payment_providers.length ? (
        <span className="font-mono text-xs">
          {row.original.payment_providers.join(", ")}
        </span>
      ) : (
        <span className="text-xs text-muted-foreground">none</span>
      ),
    meta: { className: "text-right" },
  },
]

export function StoreRegions({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(["regions"], "/admin/regions", region, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: t("frame.regions"),
        description: t("frame.regionsWhy"),
        actions: (
          <Button
            size="sm"
            nativeButton={false}
            render={<Link to="/store/regions/new" />}
          >
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
            New region
          </Button>
        ),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/store/regions/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{
        title: t("empty.regions"),
        description: t("empty.regionsWhy"),
      }}
    />
  )
}
