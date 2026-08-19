import { useState } from "react"

import { product, productStatus, type Product, type ProductStatus } from "@/api/views"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/screens/page-heading"

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
          <div className="text-muted-foreground truncate text-xs">
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
      <Badge variant={HIDDEN.includes(row.original.status) ? "outline" : "default"}>
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

export function Products() {
  const [status, setStatus] = useState<ProductStatus | "all">("all")
  const paged = usePagedList(["products", status], "/admin/products", product, {
    status: status === "all" ? undefined : status,
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Products"
        subtitle="This surface sees every status. The storefront sees published only."
      >
        <Select
          value={status}
          onValueChange={(v) => {
            setStatus(v as ProductStatus | "all")
            paged.reset()
          }}
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
      </PageHeading>

      <DataTable
        paged={paged}
        columns={columns}
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
