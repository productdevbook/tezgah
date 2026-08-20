import { inventoryItem } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function InventoryItemDetail({ id }: { id: string }) {
  const result = useDetail(
    ["inventory-items"],
    "/admin/inventory-items/{id}",
    inventoryItem,
    id
  )

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No inventory item", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader
              back="inventory"
              title={item.title ?? "Untitled item"}
              subtitle={item.sku ?? undefined}
            >
              <Badge variant={item.requires_shipping ? "default" : "outline"}>
                {item.requires_shipping ? "shipped" : "digital"}
              </Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="SKU">{item.sku ?? <Empty />}</DetailField>
                  <DetailField label="Title">{item.title ?? <Empty />}</DetailField>
                  <DetailField label="Ships">
                    {item.requires_shipping ? "shipped" : "digital, no shipping"}
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
