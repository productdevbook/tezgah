import { Link } from "@tanstack/react-router"

import { salesChannel } from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function SalesChannelDetail({ id }: { id: string }) {
  const result = useDetail(
    ["sales-channels"],
    "/admin/sales-channels/{id}",
    salesChannel,
    id
  )

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No sales channel", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="sales channels" title={item.name}>
              <Badge variant={item.is_disabled ? "outline" : "default"}>
                {item.is_disabled ? "disabled" : "selling"}
              </Badge>
              <Button
                variant="outline"
                size="sm"
                nativeButton={false}
                render={
                  <Link to="/store/sales-channels/$id/edit" params={{ id: item.id }} />
                }
              >
                Edit
              </Button>
              <DeleteAction
                path="/admin/sales-channels/{id}"
                params={{ id: item.id }}
                invalidateKey={["sales-channels"]}
                kind="sales channel"
                name={item.name}
              />
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Name">{item.name}</DetailField>
                  <DetailField label="State">
                    {item.is_disabled ? "disabled" : "selling"}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Description" full>
                    {item.description ?? <Empty />}
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
