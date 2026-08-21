import { Link } from "@tanstack/react-router"

import { giftCard, type GiftCard } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { PageHeading } from "@/components/page-heading"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { useT } from "@/panel/i18n"

const columns: Columns<GiftCard> = [
  {
    header: "field.balance",
    accessorKey: "balance",
    cell: ({ row }) =>
      `${row.original.balance} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.initialBalance",
    accessorKey: "initial_balance",
    cell: ({ row }) => row.original.initial_balance,
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "field.status",
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
  state,
  onAfterChange,
  onStateChange,
}: {
  after: string | undefined
  state: "all" | "live" | "disabled" | "spent"
  onAfterChange: (after: string | undefined) => void
  onStateChange: (state: "all" | "live" | "disabled" | "spent") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["gift-cards", state],
    "/admin/gift-cards",
    giftCard,
    {
      after,
      onAfterChange,
      query: {
        // Three narrowings over two columns, because that is what the row
        // holds: a card is stopped by hand or it is not, and it has money left
        // or it has not. There is no search — the code is stored hashed.
        disabled:
          state === "disabled"
            ? "true"
            : state === "live"
              ? "false"
              : undefined,
        spent: state === "spent" ? "true" : undefined,
        count: "true",
      },
    }
  )

  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.credit.title")}
        subtitle={t("screen.credit.subtitle")}
      >
        <Select
          value={state}
          onValueChange={(value) =>
            onStateChange(value as "all" | "live" | "disabled" | "spent")
          }
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("filter.anyCard")}</SelectItem>
            <SelectItem value="live">{t("filter.spendable")}</SelectItem>
            <SelectItem value="disabled">{t("filter.stopped")}</SelectItem>
            <SelectItem value="spent">{t("filter.spent")}</SelectItem>
          </SelectContent>
        </Select>
      </PageHeading>
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
