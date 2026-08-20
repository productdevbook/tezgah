import { product } from "@/api/schemas"
import { DetailField, FieldGrid, Metadata, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function ProductDetail({ id }: { id: string }) {
  const result = useDetail(["products"], "/admin/products/{id}", product, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No product", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="products" title={item.title} subtitle={item.handle}>
              <Badge variant={item.status === "published" ? "default" : "outline"}>
                {item.status}
              </Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Handle">{item.handle}</DetailField>
                  <DetailField label="Status">{item.status}</DetailField>
                  <DetailField label="Rejected reason">
                    {item.rejected_reason ?? <Empty />}
                  </DetailField>
                  <DetailField label="Subtitle">{item.subtitle ?? <Empty />}</DetailField>
                  <DetailField label="Discountable">
                    {item.is_discountable ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Product type">
                    {item.product_type_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Collection">
                    {item.product_collection_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Thumbnail">
                    {item.thumbnail_url ?? <Empty />}
                  </DetailField>
                  <DetailField label="External ID">
                    {item.external_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Weight">{item.weight ?? <Empty />}</DetailField>
                  <DetailField label="Length">{item.length ?? <Empty />}</DetailField>
                  <DetailField label="Height">{item.height ?? <Empty />}</DetailField>
                  <DetailField label="Width">{item.width ?? <Empty />}</DetailField>
                  <DetailField label="Material">{item.material ?? <Empty />}</DetailField>
                  <DetailField label="HS code">{item.hs_code ?? <Empty />}</DetailField>
                  <DetailField label="Origin country">
                    {item.origin_country ?? <Empty />}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Updated">{dateTime(item.updated_at)}</DetailField>
                  <DetailField label="Description" full>
                    {item.description ?? <Empty />}
                  </DetailField>
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
