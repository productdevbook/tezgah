import { Link } from "@tanstack/react-router"

import { cart, order, orderBasket, type Cart, type Order } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"
import { usePagedList } from "@/lib/paged"

export function BasketDetail({
  id,
  cartsAfter,
  ordersAfter,
  onCartsAfterChange,
  onOrdersAfterChange,
}: {
  id: string
  cartsAfter: string | undefined
  ordersAfter: string | undefined
  onCartsAfterChange: (after: string | undefined) => void
  onOrdersAfterChange: (after: string | undefined) => void
}) {
  const result = useDetail(["baskets"], "/admin/order-baskets/{id}", orderBasket, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState query={result} empty={{ title: "No basket", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader
              back="baskets"
              title={item.display_id ? `Basket #${item.display_id}` : "Basket"}
              subtitle={item.email ?? undefined}
            >
              <Badge variant={item.completed_at ? "default" : "outline"}>
                {item.completed_at ? "completed" : "open"}
              </Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Display number">
                    {item.display_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Customer">
                    {item.customer_id ? (
                      <span className="font-mono text-xs">{item.customer_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Email">{item.email ?? <Empty />}</DetailField>
                  <DetailField label="Currency">
                    <span className="font-mono text-xs uppercase">{item.currency_code}</span>
                  </DetailField>
                  <DetailField label="Payment collection">
                    {item.payment_collection_id ? (
                      <span className="font-mono text-xs">{item.payment_collection_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Completed">
                    {item.completed_at ? dateTime(item.completed_at) : <Empty />}
                  </DetailField>
                </FieldGrid>
              </CardContent>
            </Card>

            <Card>
              <CardContent className="space-y-3">
                <h2 className="text-sm font-medium">Orders</h2>
                <BasketOrders
                  basketId={id}
                  after={ordersAfter}
                  onAfterChange={onOrdersAfterChange}
                />
              </CardContent>
            </Card>

            <Card>
              <CardContent className="space-y-3">
                <h2 className="text-sm font-medium">Carts</h2>
                <BasketCarts basketId={id} after={cartsAfter} onAfterChange={onCartsAfterChange} />
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
  )
}

const orderColumns: Columns<Order> = [
  {
    header: "No",
    accessorKey: "display_id",
    cell: ({ row }) => row.original.display_id ?? "—",
    meta: { className: "w-20 text-muted-foreground font-mono text-xs" },
  },
  { header: "Status", accessorKey: "status" },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
]

function BasketOrders({
  basketId,
  after,
  onAfterChange,
}: {
  basketId: string
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["baskets", basketId, "orders"],
    "/admin/order-baskets/{id}/orders",
    order,
    { after, onAfterChange, params: { id: basketId } }
  )

  return (
    <DataTable
      paged={paged}
      columns={orderColumns}
      rowLink={(row) => (
        <Link
          to="/orders/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open order ${row.display_id ?? row.id}`}
        />
      )}
      empty={{ title: "No orders", description: "This basket has not split into an order yet." }}
    />
  )
}

const cartColumns: Columns<Cart> = [
  {
    header: "Email",
    accessorKey: "email",
    cell: ({ row }) =>
      row.original.email ?? <span className="text-muted-foreground">no email</span>,
  },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "Completed",
    accessorKey: "completed_at",
    cell: ({ row }) => (row.original.completed_at ? dateTime(row.original.completed_at) : "—"),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

function BasketCarts({
  basketId,
  after,
  onAfterChange,
}: {
  basketId: string
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["baskets", basketId, "carts"],
    "/admin/order-baskets/{id}/carts",
    cart,
    { after, onAfterChange, params: { id: basketId } }
  )

  return (
    <DataTable
      paged={paged}
      columns={cartColumns}
      empty={{
        title: "No carts",
        description: "No seller-scope has an open leg of this checkout right now.",
      }}
    />
  )
}
