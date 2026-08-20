import { cart, type Cart } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { PageHeading } from "@/components/page-heading"
import { Badge } from "@/components/ui/badge"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<Cart> = [
  {
    header: "Email",
    accessorKey: "email",
    cell: ({ row }) => row.original.email ?? <Empty />,
  },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "Region",
    accessorKey: "region_id",
    cell: ({ row }) =>
      row.original.region_id ? (
        <span className="font-mono text-xs">{row.original.region_id}</span>
      ) : (
        <Empty />
      ),
  },
  {
    header: "Status",
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
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["carts"], "/admin/carts", cart, { after, onAfterChange })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Carts"
        subtitle="Every cart the store holds, abandoned ones included."
      />
      <DataTable
        paged={paged}
        columns={columns}
        empty={{ title: "No carts", description: "Nobody has started one yet." }}
      />
    </div>
  )
}
