import { order } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

const SETTLED = ["captured", "paid", "refunded", "fulfilled", "shipped", "delivered"]
const STUCK = ["canceled", "cancelled", "requires_more", "failed", "declined"]

function tone(status: string): "default" | "outline" | "destructive" {
  if (STUCK.includes(status)) return "destructive"
  if (SETTLED.includes(status)) return "default"
  return "outline"
}

export function OrderDetail({ id }: { id: string }) {
  const result = useDetail(["orders"], "/admin/orders/{id}", order, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState query={result} empty={{ title: "No order", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader
              back="orders"
              title={item.display_id ? `Order #${item.display_id}` : "Order"}
              subtitle={item.email ?? undefined}
            >
              {item.is_draft ? <Badge variant="outline">draft</Badge> : null}
            </DetailHeader>
            <Card>
              <CardContent>
                {/*
                  An order carries three statuses that move independently —
                  itself, its money and its parcels — so all three get their
                  own field rather than folding into one.
                */}
                <FieldGrid>
                  <DetailField label="Order status">
                    <Badge variant={tone(item.status)}>{item.status}</Badge>
                  </DetailField>
                  <DetailField label="Payment status">
                    <Badge variant={tone(item.payment_status)}>{item.payment_status}</Badge>
                  </DetailField>
                  <DetailField label="Fulfilment status">
                    <Badge variant={tone(item.fulfillment_status)}>
                      {item.fulfillment_status}
                    </Badge>
                  </DetailField>
                  <DetailField label="Draft">{item.is_draft ? "yes" : "no"}</DetailField>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Display number">
                    {item.display_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Email">{item.email ?? <Empty />}</DetailField>
                  <DetailField label="Currency">
                    <span className="font-mono text-xs uppercase">{item.currency_code}</span>
                  </DetailField>
                  <DetailField label="Version">{item.version}</DetailField>
                  <DetailField label="Payment collection">
                    {item.payment_collection_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Basket">{item.basket_id ?? <Empty />}</DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Completed">
                    {item.completed_at ? dateTime(item.completed_at) : <Empty />}
                  </DetailField>
                  <DetailField label="Canceled">
                    {item.canceled_at ? dateTime(item.canceled_at) : <Empty />}
                  </DetailField>
                </FieldGrid>
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
  )
}
