import { Link } from "@tanstack/react-router"

import { promotion } from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { DetailField, FieldGrid, Metadata, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function PromotionDetail({ id }: { id: string }) {
  const result = useDetail(["promotions"], "/admin/promotions/{id}", promotion, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No promotion", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="promotions" title={item.code} subtitle={item.kind}>
              <Badge variant={item.status === "active" ? "default" : "outline"}>
                {item.status}
              </Badge>
              {item.is_automatic ? <Badge variant="outline">automatic</Badge> : null}
              <Button
                variant="outline"
                size="sm"
                nativeButton={false}
                render={<Link to="/promotions/$id/edit" params={{ id: item.id }} />}
              >
                Edit
              </Button>
              <DeleteAction
                path="/admin/promotions/{id}"
                params={{ id: item.id }}
                invalidateKey={["promotions"]}
                kind="promotion"
                name={item.code}
              />
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Code">
                    <span className="font-mono text-xs">{item.code}</span>
                  </DetailField>
                  <DetailField label="Kind">{item.kind}</DetailField>
                  <DetailField label="Status">{item.status}</DetailField>
                  <DetailField label="Automatic">
                    {item.is_automatic ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Campaign">
                    {item.campaign_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Used">
                    {/* Claimed at checkout, not counted at payment: what is spoken for. */}
                    {item.usage_limit === null
                      ? `${item.used}`
                      : `${item.used} / ${item.usage_limit}`}
                  </DetailField>
                  <DetailField label="Per-customer limit">
                    {item.customer_usage_limit ?? <Empty />}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Metadata" full>
                    <Metadata value={item.metadata} />
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
