import { Link } from "@tanstack/react-router"

import { region } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function RegionDetail({ id }: { id: string }) {
  const result = useDetail(["regions"], "/admin/regions/{id}", region, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState query={result} empty={{ title: "No region", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader back="regions" title={item.name}>
              <Badge variant="outline">
                {item.is_tax_inclusive ? "tax inclusive" : "tax exclusive"}
              </Badge>
              {item.has_automatic_taxes ? <Badge>automatic taxes</Badge> : null}
              <Button
                variant="outline"
                size="sm"
                nativeButton={false}
                render={<Link to="/store/regions/$id/edit" params={{ id: item.id }} />}
              >
                Edit
              </Button>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Name">{item.name}</DetailField>
                  <DetailField label="Currency">
                    <span className="font-mono text-xs uppercase">{item.currency_code}</span>
                  </DetailField>
                  <DetailField label="Tax inclusive">
                    {item.is_tax_inclusive ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Automatic taxes">
                    {item.has_automatic_taxes ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Payment providers" full>
                    {item.payment_providers.length ? (
                      <span className="font-mono text-xs">
                        {item.payment_providers.join(", ")}
                      </span>
                    ) : (
                      <Empty />
                    )}
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
