import { shippingOption } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function ShippingOptionDetail({ id }: { id: string }) {
  const result = useDetail(["shipping-options"], "/admin/shipping-options/{id}", shippingOption, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No shipping option", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="shipping options" title={item.name}>
              <Badge variant="outline">{item.price_type}</Badge>
              {item.is_return ? <Badge>return</Badge> : null}
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Service zone">
                    <span className="font-mono text-xs">{item.service_zone_id}</span>
                  </DetailField>
                  <DetailField label="Shipping profile">
                    {item.shipping_profile_id ? (
                      <span className="font-mono text-xs">{item.shipping_profile_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Option type">
                    {item.shipping_option_type_id ? (
                      <span className="font-mono text-xs">{item.shipping_option_type_id}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="In store">
                    {item.enabled_in_store ? "enabled" : "disabled"}
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
