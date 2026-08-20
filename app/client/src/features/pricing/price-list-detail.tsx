import { priceList } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function PriceListDetail({ id }: { id: string }) {
  const result = useDetail(["price-lists"], "/admin/price-lists/{id}", priceList, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No price list", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="price lists" title={item.title}>
              <Badge variant="outline">{item.kind}</Badge>
              <Badge>{item.status}</Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Description">{item.description ?? <Empty />}</DetailField>
                  <DetailField label="Rules">{item.rules_count}</DetailField>
                  <DetailField label="Starts">
                    {item.starts_at ? dateTime(item.starts_at) : <Empty />}
                  </DetailField>
                  <DetailField label="Ends">
                    {item.ends_at ? dateTime(item.ends_at) : <Empty />}
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
