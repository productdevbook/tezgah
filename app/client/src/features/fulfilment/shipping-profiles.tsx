import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { shippingProfile, type ShippingProfile } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"

const columns: Columns<ShippingProfile> = [
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

export function ShippingProfiles({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["shipping-profiles"],
    "/admin/shipping-profiles",
    shippingProfile,
    {
      after,
      onAfterChange,
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.shippingProfiles"),
        description: t("frame.shippingProfilesWhy"),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/fulfilment/shipping-profiles/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{
        title: t("empty.shippingProfiles"),
        description: t("empty.shippingProfilesWhy"),
      }}
    />
  )
}
