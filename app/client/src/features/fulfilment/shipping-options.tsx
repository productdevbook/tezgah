import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { shippingOption, type ShippingOption } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { SearchBox } from "@/components/search-box"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

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
  q,
  kind,
  onAfterChange,
  onQChange,
  onKindChange,
}: {
  after: string | undefined
  q: string | undefined
  kind: "all" | "outbound" | "return"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onKindChange: (kind: "all" | "outbound" | "return") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["shipping-options", q ?? "", kind],
    "/admin/shipping-options",
    shippingOption,
    {
      after,
      onAfterChange,
      query: {
        q,
        // An option a shopper picks and an option a return travels back on
        // are both rows here; `is_return` is what tells them apart.
        is_return:
          kind === "return"
            ? "true"
            : kind === "outbound"
              ? "false"
              : undefined,
        count: "true",
      },
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.shippingOptions"),
        description: t("frame.shippingOptionsWhy"),
        actions: (
          <div className="flex items-center gap-2">
            <SearchBox
              value={q}
              onChange={onQChange}
              placeholder={t("search.shippingOptions")}
            />
            <Select
              value={kind}
              onValueChange={(value) =>
                onKindChange(value as "all" | "outbound" | "return")
              }
            >
              <SelectTrigger className="w-40" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("filter.anyOption")}</SelectItem>
                <SelectItem value="outbound">{t("filter.outbound")}</SelectItem>
                <SelectItem value="return">{t("filter.forReturns")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
        ),
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
