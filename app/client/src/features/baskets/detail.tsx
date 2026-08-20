import { Link } from "@tanstack/react-router"

import { cart, order, orderBasket, type Cart, type Order } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import {
  Section,
  SectionBody,
  SectionRow,
  SectionRows,
} from "@/components/section"
import { Badge } from "@/components/ui/badge"
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
  const result = useDetail(
    ["baskets"],
    "/admin/order-baskets/{id}",
    orderBasket,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No basket", description: "Nothing to show." }}
      back="baskets"
      title={(item) =>
        item.display_id ? `Basket #${item.display_id}` : "Basket"
      }
      subtitle={(item) => item.email ?? undefined}
      actions={(item) => (
        <Badge variant={item.completed_at ? "default" : "outline"}>
          {item.completed_at ? "completed" : "open"}
        </Badge>
      )}
      main={() => (
        <>
          <Section
            title="Orders"
            description="One basket becomes one order per seller — the payment is single, the fulfilment is not."
          >
            <SectionBody>
              <BasketOrders
                basketId={id}
                after={ordersAfter}
                onAfterChange={onOrdersAfterChange}
              />
            </SectionBody>
          </Section>

          <Section
            title="Carts"
            description="A seller's own leg of the checkout, before it became an order."
          >
            <SectionBody>
              <BasketCarts
                basketId={id}
                after={cartsAfter}
                onAfterChange={onCartsAfterChange}
              />
            </SectionBody>
          </Section>
        </>
      )}
      side={(item) => (
        <>
          <Section title="Who it is for">
            <SectionRows>
              <SectionRow label="Email" value={item.email} />
              <SectionRow
                label="Customer"
                value={
                  item.customer_id ? <Mono>{item.customer_id}</Mono> : null
                }
              />
              <SectionRow
                label="Currency"
                value={<Mono>{item.currency_code.toUpperCase()}</Mono>}
              />
            </SectionRows>
          </Section>

          <Section title="The payment">
            <SectionRows>
              <SectionRow
                label="Collection"
                value={
                  item.payment_collection_id ? (
                    <Mono>{item.payment_collection_id}</Mono>
                  ) : null
                }
              />
              <SectionRow
                label="Completed"
                value={item.completed_at ? dateTime(item.completed_at) : null}
              />
            </SectionRows>
          </Section>

          <Section title="Details">
            <SectionRows>
              <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
              <SectionRow label="Number" value={item.display_id} />
            </SectionRows>
          </Section>
        </>
      )}
    />
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
      empty={{
        title: "No orders",
        description: "This basket has not split into an order yet.",
      }}
    />
  )
}

const cartColumns: Columns<Cart> = [
  {
    header: "Email",
    accessorKey: "email",
    cell: ({ row }) =>
      row.original.email ?? (
        <span className="text-muted-foreground">no email</span>
      ),
  },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "Completed",
    accessorKey: "completed_at",
    cell: ({ row }) =>
      row.original.completed_at ? dateTime(row.original.completed_at) : "—",
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
        description:
          "No seller-scope has an open leg of this checkout right now.",
      }}
    />
  )
}
