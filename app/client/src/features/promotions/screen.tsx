import { Link } from "@tanstack/react-router"

import { promotion, type Promotion } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { SearchBox } from "@/components/search-box"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useT } from "@/panel/i18n"

import {
  PROMOTION_STATUS,
  type PromotionStatus,
} from "@/features/promotions/status"

const columns: Columns<Promotion> = [
  {
    header: "field.code",
    accessorKey: "code",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <span className="font-mono text-xs">{row.original.code}</span>
        {row.original.is_automatic ? (
          <Badge variant="outline">automatic</Badge>
        ) : null}
      </div>
    ),
  },
  { header: "field.kind", accessorKey: "kind", meta: { className: "text-sm" } },
  {
    header: "field.status",
    accessorKey: "status",
    cell: ({ row }) => (
      <Badge variant={row.original.status === "active" ? "default" : "outline"}>
        {row.original.status}
      </Badge>
    ),
  },
  {
    header: "field.used",
    accessorKey: "used",
    // Claimed at checkout, not counted at payment: this is what is spoken for.
    cell: ({ row }) =>
      row.original.usage_limit === null
        ? `${row.original.used}`
        : `${row.original.used} / ${row.original.usage_limit}`,
    meta: { className: "text-right font-mono text-xs" },
  },
]

export function Promotions({
  after,
  q,
  status,
  onAfterChange,
  onQChange,
  onStatusChange,
}: {
  after: string | undefined
  q: string | undefined
  status: PromotionStatus | "all"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onStatusChange: (status: PromotionStatus | "all") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["promotions", q ?? "", status],
    "/admin/promotions",
    promotion,
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
    <div className="space-y-4">
      <PageHeading
        title={t("screen.promotions.title")}
        subtitle={t("screen.promotions.subtitle")}
      >
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder={t("search.promotions")}
        />
        <Select
          value={status}
          onValueChange={(value) =>
            onStatusChange(value as PromotionStatus | "all")
          }
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder={t("filter.anyStatus")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("filter.anyStatus")}</SelectItem>
            {PROMOTION_STATUS.map((one) => (
              <SelectItem key={one} value={one}>
                {one}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </PageHeading>
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/promotions/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open ${row.code}`}
          />
        )}
        empty={{
          title: t("screen.promotions.empty"),
          description: q
            ? t("search.nothingMatches", { q })
            : t("screen.promotions.emptyAny"),
        }}
      />
    </div>
  )
}
