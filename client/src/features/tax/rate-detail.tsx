import { taxRate } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function TaxRateDetail({ id }: { id: string }) {
  const result = useDetail(["tax-rates"], "/admin/tax-rates/{id}", taxRate, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState query={result} empty={{ title: "No tax rate", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader back="tax rates" title={item.name}>
              {item.is_default ? <Badge>default</Badge> : null}
              {item.is_combinable ? <Badge variant="outline">combinable</Badge> : null}
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Code">{item.code ?? <Empty />}</DetailField>
                  <DetailField label="Rate">
                    <span className="font-mono text-xs">{item.rate}</span>
                  </DetailField>
                  <DetailField label="Tax region">
                    <span className="font-mono text-xs">{item.tax_region_id}</span>
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
