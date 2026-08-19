import { useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { get } from "@/api/client"
import type { InventoryItem, Page } from "@/api/views"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { PageHeading } from "@/screens/page-heading"
import { Pager } from "@/screens/products"

export function Inventory() {
  const [cursors, setCursors] = useState<string[]>([])
  const after = cursors.at(-1)

  const query = useQuery({
    queryKey: ["inventory-items", after],
    queryFn: ({ signal }) =>
      get<Page<InventoryItem>>("/admin/inventory-items", {
        signal,
        query: { limit: 25, after },
      }),
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Inventory"
        subtitle="An item is the thing counted. What is on hand is counted per location, one level down."
      />

      <QueryState
        query={query}
        empty={{
          title: "Nothing stocked",
          description: "No inventory item exists yet.",
        }}
      >
        {(page) => (
          <>
            <div className="rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>SKU</TableHead>
                    <TableHead>Title</TableHead>
                    <TableHead className="text-right">Ships</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.items.map((item) => (
                    <TableRow key={item.id}>
                      <TableCell className="font-mono text-xs">
                        {item.sku ?? (
                          <span className="text-muted-foreground">none</span>
                        )}
                      </TableCell>
                      <TableCell className="max-w-96 truncate">
                        {item.title ?? (
                          <span className="text-muted-foreground">untitled</span>
                        )}
                      </TableCell>
                      <TableCell className="text-right">
                        <Badge
                          variant={item.requires_shipping ? "default" : "outline"}
                        >
                          {item.requires_shipping ? "shipped" : "digital"}
                        </Badge>
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
