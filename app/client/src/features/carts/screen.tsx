import { cart, type Cart } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { PageHeading } from "@/components/page-heading"
import { Badge } from "@/components/ui/badge"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"
import { SearchBox } from "@/components/search-box"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useT } from "@/panel/i18n"

const columns: Columns<Cart> = [
  {
    header: "field.email",
    accessorKey: "email",
    cell: ({ row }) => row.original.email ?? <Empty />,
  },
  {
    header: "field.currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "field.region",
    accessorKey: "region_id",
    cell: ({ row }) =>
      row.original.region_id ? (
        <span className="font-mono text-xs">{row.original.region_id}</span>
      ) : (
        <Empty />
      ),
  },
  {
    header: "field.status",
    accessorKey: "completed_at",
    cell: ({ row }) => (
      <Badge variant={row.original.completed_at ? "default" : "outline"}>
        {row.original.completed_at ? "completed" : "open"}
      </Badge>
    ),
    meta: { className: "text-right" },
  },
]

/**
 * `cart::list` (`GET /admin/carts`) was written and, until now, reachable
 * from nothing — `tests/reachable.rs`'s tolerated list carried it under "the
 * back office has no cart screen". This is the screen that makes that
 * reason false. There is no `GET /admin/carts/{id}` — only a shopper's own
 * cart is fetched by id, at `/store/carts/{id}` — so a row here goes
 * nowhere.
 */
export function Carts({
  after,
  q,
  state,
  onAfterChange,
  onQChange,
  onStateChange,
}: {
  after: string | undefined
  q: string | undefined
  state: "all" | "open" | "completed"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onStateChange: (state: "all" | "open" | "completed") => void
}) {
  const t = useT()
  const paged = usePagedList(["carts", q ?? "", state], "/admin/carts", cart, {
    after,
    onAfterChange,
    query: {
      q,
      completed:
        state === "completed" ? "true" : state === "open" ? "false" : undefined,
      count: "true",
    },
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.carts.title")}
        subtitle={t("screen.carts.subtitle")}
      >
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder={t("search.carts")}
        />
        <Select
          value={state}
          onValueChange={(value) =>
            onStateChange(value as "all" | "open" | "completed")
          }
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("filter.anyCart")}</SelectItem>
            <SelectItem value="open">{t("filter.stillOpen")}</SelectItem>
            <SelectItem value="completed">{t("filter.ordered")}</SelectItem>
          </SelectContent>
        </Select>
      </PageHeading>
      <DataTable
        paged={paged}
        columns={columns}
        empty={{
          title: t("screen.carts.empty"),
          description: q
            ? t("search.nothingMatches", { q })
            : t("screen.carts.emptyAny"),
        }}
      />
    </div>
  )
}
