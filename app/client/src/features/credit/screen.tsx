import { Link } from "@tanstack/react-router"

import { giftCard, type GiftCard } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { PageHeading } from "@/components/page-heading"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { useT } from "@/panel/i18n"

const columns: Columns<GiftCard> = [
  {
    header: "Balance",
    accessorKey: "balance",
    cell: ({ row }) =>
      `${row.original.balance} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Initial balance",
    accessorKey: "initial_balance",
    cell: ({ row }) => row.original.initial_balance,
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "Status",
    accessorKey: "disabled_at",
    cell: ({ row }) => (
      <Badge variant={row.original.disabled_at ? "outline" : "default"}>
        {row.original.disabled_at ? "disabled" : "active"}
      </Badge>
    ),
    meta: { className: "text-right" },
  },
]

/**
 * `cart::list` was written and reachable from nothing but `carts` (see
 * `features/carts/screen.tsx`) — gift cards, by contrast, have always had a
 * screen-shaped surface: `GET /admin/gift-cards` and `GET .../{id}` are both
 * bound. A single list, no tabs — see `lib/nav.ts`'s `credit` section.
 */
export function GiftCards({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(["gift-cards"], "/admin/gift-cards", giftCard, {
    after,
    onAfterChange,
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.credit.title")}
        subtitle={t("screen.credit.subtitle")}
      />
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/credit/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open gift card ${row.id}`}
          />
        )}
        empty={{
          title: t("screen.credit.empty"),
          description: t("screen.credit.emptyAny"),
        }}
      />
    </div>
  )
}
