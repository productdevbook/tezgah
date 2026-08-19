import { useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { get } from "@/api/client"
import type { Order, Page } from "@/api/views"
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

/**
 * An order carries three statuses that move independently — the order itself,
 * its money and its parcels — so the table shows all three rather than folding
 * them into one word that would have to be wrong about two of them.
 */
const SETTLED = ["captured", "paid", "refunded", "fulfilled", "shipped", "delivered"]
const STUCK = ["canceled", "cancelled", "requires_more", "failed", "declined"]

function tone(status: string): "default" | "outline" | "destructive" {
  if (STUCK.includes(status)) return "destructive"
  if (SETTLED.includes(status)) return "default"
  return "outline"
}

export function Orders() {
  const [cursors, setCursors] = useState<string[]>([])
  const after = cursors.at(-1)

  const query = useQuery({
    queryKey: ["orders", after],
    queryFn: ({ signal }) =>
      get<Page<Order>>("/admin/orders", {
        signal,
        query: { limit: 25, after },
      }),
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Orders"
        subtitle="Drafts are listed too, and say so."
      />

      <QueryState
        query={query}
        empty={{ title: "No orders", description: "Nothing has been placed yet." }}
      >
        {(page) => (
          <>
            <div className="rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead className="w-20">No</TableHead>
                    <TableHead>Customer</TableHead>
                    <TableHead>Order</TableHead>
                    <TableHead>Payment</TableHead>
                    <TableHead>Fulfilment</TableHead>
                    <TableHead className="text-right">Placed</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {page.items.map((order) => (
                    <TableRow key={order.id}>
                      <TableCell className="text-muted-foreground font-mono text-xs">
                        {order.display_id ?? "—"}
                      </TableCell>
                      <TableCell className="max-w-56 truncate">
                        {order.email ?? (
                          <span className="text-muted-foreground">no email</span>
                        )}
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1.5">
                          <Badge variant={tone(order.status)}>{order.status}</Badge>
                          {order.is_draft ? (
                            <Badge variant="outline">draft</Badge>
                          ) : null}
                        </div>
                      </TableCell>
                      <TableCell>
                        <Badge variant={tone(order.payment_status)}>
                          {order.payment_status}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <Badge variant={tone(order.fulfillment_status)}>
                          {order.fulfillment_status}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-muted-foreground text-right text-xs">
                        {new Date(order.created_at).toLocaleDateString()}
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
