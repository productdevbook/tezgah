import { useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { get } from "@/api/client"
import type { Page, Product, ProductStatus } from "@/api/views"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { PageHeading } from "@/screens/page-heading"

const STATUSES: ProductStatus[] = [
  "draft",
  "proposed",
  "published",
  "archived",
  "rejected",
]

/** Which of the five read as "not for sale", so the badge can say it once. */
const HIDDEN: ProductStatus[] = ["draft", "proposed", "rejected", "archived"]

export function Products() {
  const [status, setStatus] = useState<ProductStatus | "all">("all")
  const [cursors, setCursors] = useState<string[]>([])
  const after = cursors.at(-1)

  const query = useQuery({
    queryKey: ["products", status, after],
    queryFn: ({ signal }) =>
      get<Page<Product>>("/admin/products", {
        signal,
        query: {
          limit: 25,
          after,
          status: status === "all" ? undefined : status,
        },
      }),
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Products"
        subtitle="The admin surface sees every status. The storefront sees published only."
      >
        <Select
          value={status}
          onValueChange={(v) => {
            setStatus(v as ProductStatus | "all")
            setCursors([])
          }}
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder="Any status" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Any status</SelectItem>
            {STATUSES.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </PageHeading>

      <QueryState
        query={query}
        empty={{
          title: "No products",
          description:
            status === "all"
              ? "Nothing in the catalogue yet."
              : `Nothing with status ${status}.`,
        }}
      >
        {(page) => (
          <>
            <div className="rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Title</TableHead>
                    <TableHead>Handle</TableHead>
                    <TableHead>Status</TableHead>
                    <TableHead className="text-right">Discountable</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.items.map((product) => (
                    <TableRow key={product.id}>
                      <TableCell className="font-medium">
                        <div className="truncate">{product.title}</div>
                        {product.subtitle ? (
                          <div className="text-muted-foreground truncate text-xs">
                            {product.subtitle}
                          </div>
                        ) : null}
                      </TableCell>
                      <TableCell className="text-muted-foreground font-mono text-xs">
                        {product.handle}
                      </TableCell>
                      <TableCell>
                        <Badge
                          variant={
                            HIDDEN.includes(product.status)
                              ? "outline"
                              : "default"
                          }
                        >
                          {product.status}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-muted-foreground text-right text-xs">
                        {product.is_discountable ? "yes" : "no"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            <Pager
              hasPrevious={cursors.length > 0}
              next={page.next}
              onBack={() => setCursors((c) => c.slice(0, -1))}
              onNext={(cursor) => setCursors((c) => [...c, cursor])}
            />
          </>
        )}
      </QueryState>
    </div>
  )
}

/**
 * Cursor paging, so "back" is a stack rather than an offset: the API hands out
 * a cursor for the next page and nothing that walks backwards.
 */
export function Pager({
  hasPrevious,
  next,
  onBack,
  onNext,
}: {
  hasPrevious: boolean
  next: string | null
  onBack: () => void
  onNext: (cursor: string) => void
}) {
  if (!hasPrevious && !next) return null
  return (
    <div className="flex items-center justify-end gap-2">
      <Button
        variant="outline"
        size="sm"
        disabled={!hasPrevious}
        onClick={onBack}
      >
        Back
      </Button>
      <Button
        variant="outline"
        size="sm"
        disabled={!next}
        onClick={() => next && onNext(next)}
      >
        Next
      </Button>
    </div>
  )
}
