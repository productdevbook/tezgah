import { Link } from "@tanstack/react-router"

import { order, type Order } from "@/api/schemas"
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

/**
 * An order carries three statuses that move independently — itself, its money
 * and its parcels — so all three are shown. Folding them into one word would
 * have to be wrong about two of them.
 */
const SETTLED = [
  "captured",
  "paid",
  "refunded",
  "fulfilled",
  "shipped",
  "delivered",
]
const STUCK = ["canceled", "cancelled", "requires_more", "failed", "declined"]

function tone(status: string): "default" | "outline" | "destructive" {
  if (STUCK.includes(status)) return "destructive"
  if (SETTLED.includes(status)) return "default"
  return "outline"
}

const columns: Columns<Order> = [
  {
    header: "field.no",
    accessorKey: "display_id",
    cell: ({ row }) => row.original.display_id ?? "—",
    meta: { className: "w-20 text-muted-foreground font-mono text-xs" },
  },
  {
    header: "field.customer",
    accessorKey: "email",
    cell: ({ row }) =>
      row.original.email ?? (
        <span className="text-muted-foreground">no email</span>
      ),
    meta: { className: "max-w-56 truncate" },
  },
  {
    header: "field.order",
    accessorKey: "status",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant={tone(row.original.status)}>{row.original.status}</Badge>
        {row.original.is_draft ? <Badge variant="outline">draft</Badge> : null}
      </div>
    ),
  },
  {
    header: "field.payment",
    accessorKey: "payment_status",
    cell: ({ row }) => (
      <Badge variant={tone(row.original.payment_status)}>
        {row.original.payment_status}
      </Badge>
    ),
  },
  {
    header: "field.fulfilment",
    accessorKey: "fulfillment_status",
    cell: ({ row }) => (
      <Badge variant={tone(row.original.fulfillment_status)}>
        {row.original.fulfillment_status}
      </Badge>
    ),
  },
  {
    header: "field.placed",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleDateString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Orders({
  after,
  q,
  by,
  onAfterChange,
  onQChange,
  onByChange,
}: {
  after: string | undefined
  q: string | undefined
  by: "created" | "email"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onByChange: (by: "created" | "email") => void
}) {
  const t = useT()
  const paged = usePagedList(["orders", q ?? "", by], "/admin/orders", order, {
    after,
    onAfterChange,
    query: { q, by, count: "true" },
  })
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.orders.title")}
        subtitle={t("screen.orders.subtitle")}
      >
        <Select
          value={by}
          onValueChange={(value) => onByChange(value as "created" | "email")}
        >
          <SelectTrigger className="w-36" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {/* Two, because the crate orders this list two ways. */}
            <SelectItem value="created">{t("sort.newest")}</SelectItem>
            <SelectItem value="email">{t("sort.byEmail")}</SelectItem>
          </SelectContent>
        </Select>
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder={t("search.orders")}
        />
      </PageHeading>
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/orders/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open order ${row.display_id ?? row.id}`}
          />
        )}
        empty={{
          title: t("screen.orders.empty"),
          description: q
            ? t("search.nothingMatches", { q })
            : t("screen.orders.emptyAny"),
        }}
      />
    </div>
  )
}
