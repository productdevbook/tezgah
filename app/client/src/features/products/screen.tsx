import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"

import {
  product,
  productStatus,
  type Product,
  type ProductStatus,
} from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { SearchBox } from "@/components/search-box"

/** Four of the five mean "not for sale", for four different reasons. */
const HIDDEN: ProductStatus[] = ["draft", "proposed", "rejected", "archived"]

const columns: Columns<Product> = [
  {
    header: "Title",
    accessorKey: "title",
    cell: ({ row }) => (
      <div className="min-w-0">
        <div className="truncate font-medium">{row.original.title}</div>
        {row.original.subtitle ? (
          <div className="truncate text-xs text-muted-foreground">
            {row.original.subtitle}
          </div>
        ) : null}
      </div>
    ),
  },
  {
    header: "Handle",
    accessorKey: "handle",
    meta: { className: "text-muted-foreground font-mono text-xs" },
  },
  {
    header: "Status",
    accessorKey: "status",
    cell: ({ row }) => (
      <Badge
        variant={HIDDEN.includes(row.original.status) ? "outline" : "default"}
      >
        {row.original.status}
      </Badge>
    ),
  },
  {
    header: "Discountable",
    accessorKey: "is_discountable",
    cell: ({ row }) => (row.original.is_discountable ? "yes" : "no"),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Products({
  status,
  after,
  q,
  onStatusChange,
  onAfterChange,
  onQChange,
}: {
  status: ProductStatus | "all"
  after: string | undefined
  q: string | undefined
  onStatusChange: (status: ProductStatus | "all") => void
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
}) {
  const paged = usePagedList(
    ["products", status, q ?? ""],
    "/admin/products",
    product,
    {
      after,
      onAfterChange,
      query: { status: status === "all" ? undefined : status, q },
    }
  )

  return (
    <div className="space-y-4">
      <PageHeading
        title="Products"
        subtitle="This surface sees every status. The storefront sees published only."
      >
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder="Search title, handle, subtitle"
        />
        <Select
          value={status}
          onValueChange={(v) => onStatusChange(v as ProductStatus | "all")}
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder="Any status" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Any status</SelectItem>
            {productStatus.options.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          size="sm"
          nativeButton={false}
          render={<Link to="/products/new" />}
        >
          <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          New product
        </Button>
      </PageHeading>

      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/products/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open ${row.title}`}
          />
        )}
        empty={{
          title: "No products",
          description:
            status === "all"
              ? "Nothing in the catalogue yet."
              : `Nothing with status ${status}.`,
        }}
      />
    </div>
  )
}
