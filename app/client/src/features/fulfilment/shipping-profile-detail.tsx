import { shippingProfile } from "@/api/schemas"
import { DetailField, FieldGrid } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function ShippingProfileDetail({ id }: { id: string }) {
  const result = useDetail(
    ["shipping-profiles"],
    "/admin/shipping-profiles/{id}",
    shippingProfile,
    id
  )

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No shipping profile", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="shipping profiles" title={item.name}>
              <Badge variant="outline">{item.kind}</Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Kind">{item.kind}</DetailField>
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
