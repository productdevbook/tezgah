import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { priceList, type PriceList } from "@/api/schemas"
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

const columns: Columns<PriceList> = [
  {
    header: "field.title",
    accessorKey: "title",
    meta: { className: "font-medium" },
  },
  {
    header: "field.kind",
    accessorKey: "kind",
    cell: ({ row }) => <Badge variant="outline">{row.original.kind}</Badge>,
  },
  {
    header: "field.status",
    accessorKey: "status",
    cell: ({ row }) => <Badge>{row.original.status}</Badge>,
  },
  {
    header: "field.rules",
    accessorKey: "rules_count",
    meta: { className: "text-right font-mono text-xs" },
  },
]

export function PriceLists({
  after,
  q,
  status,
  onAfterChange,
  onQChange,
  onStatusChange,
}: {
  after: string | undefined
  q: string | undefined
  status: "all" | "active" | "draft"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onStatusChange: (status: "all" | "active" | "draft") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["price-lists", q ?? "", status],
    "/admin/price-lists",
    priceList,
    {
      after,
      onAfterChange,
      query: {
        q,
        status: status === "all" ? undefined : status,
        count: "true",
      },
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.priceLists"),
        description: t("frame.priceListsWhy"),
        actions: (
          <div className="flex items-center gap-2">
            <SearchBox
              value={q}
              onChange={onQChange}
              placeholder={t("search.priceLists")}
            />
            <Select
              value={status}
              onValueChange={(value) =>
                onStatusChange(value as "all" | "active" | "draft")
              }
            >
              <SelectTrigger className="w-36" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("filter.anyStatus")}</SelectItem>
                <SelectItem value="active">active</SelectItem>
                <SelectItem value="draft">draft</SelectItem>
              </SelectContent>
            </Select>
          </div>
        ),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/pricing/price-lists/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.title}`}
        />
      )}
      empty={{
        title: t("empty.priceLists"),
        description: t("empty.priceListsWhy"),
      }}
    />
  )
}
