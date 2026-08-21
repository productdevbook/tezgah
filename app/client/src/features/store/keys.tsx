import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"

import { publishableKey, type PublishableKey } from "@/api/schemas"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { DataTable, type Columns } from "@/components/data-table"
import { usePagedList } from "@/lib/paged"

const columns: Columns<PublishableKey> = [
  {
    header: "field.title",
    accessorKey: "title",
    meta: { className: "font-medium" },
  },
  {
    header: "field.state",
    accessorKey: "revoked_at",
    cell: ({ row }) => (
      <Badge variant={row.original.revoked_at ? "outline" : "default"}>
        {row.original.revoked_at ? "revoked" : "active"}
      </Badge>
    ),
  },
  {
    header: "field.lastUsed",
    accessorKey: "last_used_at",
    cell: ({ row }) =>
      row.original.last_used_at ? (
        new Date(row.original.last_used_at).toLocaleString()
      ) : (
        <span className="text-muted-foreground">never</span>
      ),
    meta: { className: "text-muted-foreground text-xs" },
  },
  {
    header: "field.created",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleDateString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

/**
 * `POST /admin/publishable-api-keys` shows the token once, at
 * `/store/keys/new` — never stored anywhere it could be read back, so this
 * list has no token column: `GET /admin/publishable-api-keys` doesn't answer
 * with one either (`PublishableKeyView`, not `IssuedKeyView`).
 */
export function StoreKeys({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["publishable-keys"],
    "/admin/publishable-api-keys",
    publishableKey,
    { after, onAfterChange }
  )

  return (
    <DataTable
      header={{
        title: "Publishable keys",
        description:
          "A key pins a storefront to the channels it may read. The token is shown once, when it is minted.",
        actions: (
          <Button
            size="sm"
            nativeButton={false}
            render={<Link to="/store/keys/new" />}
          >
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
            Mint key
          </Button>
        ),
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: "No publishable keys",
        description:
          "What a storefront sends as x-publishable-key. Shown once when minted.",
      }}
    />
  )
}
