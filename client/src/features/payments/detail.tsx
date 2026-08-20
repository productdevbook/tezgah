import { payment } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function PaymentDetail({ id }: { id: string }) {
  const result = useDetail(["payments"], "/admin/payments/{id}", payment, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState query={result} empty={{ title: "No payment", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader
              back="payments"
              title={`${item.amount.amount} ${item.amount.currency.toUpperCase()}`}
            >
              {item.captured_at ? <Badge>captured</Badge> : null}
              {item.canceled_at ? <Badge variant="outline">canceled</Badge> : null}
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Payment collection">
                    <span className="font-mono text-xs">{item.payment_collection_id}</span>
                  </DetailField>
                  <DetailField label="Payment session">
                    {item.payment_session_id ? (
                      <span className="font-mono text-xs">{item.payment_session_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Captured">
                    {item.captured_at ? dateTime(item.captured_at) : <Empty />}
                  </DetailField>
                  <DetailField label="Canceled">
                    {item.canceled_at ? dateTime(item.canceled_at) : <Empty />}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                </FieldGrid>
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
  )
}
