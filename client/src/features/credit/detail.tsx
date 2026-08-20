import { giftCard } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function GiftCardDetail({ id }: { id: string }) {
  const result = useDetail(["gift-cards"], "/admin/gift-cards/{id}", giftCard, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No gift card", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader
              back="credit"
              title={`${item.balance} ${item.currency_code.toUpperCase()}`}
            >
              <Badge variant={item.disabled_at ? "outline" : "default"}>
                {item.disabled_at ? "disabled" : "active"}
              </Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Initial balance">
                    <span className="font-mono text-xs">
                      {item.initial_balance} {item.currency_code.toUpperCase()}
                    </span>
                  </DetailField>
                  <DetailField label="Customer">
                    {item.customer_id ? (
                      <span className="font-mono text-xs">{item.customer_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Issued on order">
                    {item.issued_order_id ? (
                      <span className="font-mono text-xs">{item.issued_order_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Expires">
                    {item.expires_at ? dateTime(item.expires_at) : <Empty />}
                  </DetailField>
                  <DetailField label="Disabled">
                    {item.disabled_at ? dateTime(item.disabled_at) : <Empty />}
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
