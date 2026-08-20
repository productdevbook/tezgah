import { taxRegion } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

export function TaxRegionDetail({ id }: { id: string }) {
  const result = useDetail(["tax-regions"], "/admin/tax-regions/{id}", taxRegion, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No tax region", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="tax regions" title={item.country_code.toUpperCase()} />
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Country">
                    <span className="font-mono text-xs uppercase">{item.country_code}</span>
                  </DetailField>
                  <DetailField label="Province">
                    {item.province_code ? (
                      <span className="font-mono text-xs uppercase">{item.province_code}</span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Provider">{item.provider ?? <Empty />}</DetailField>
                  <DetailField label="Parent region">
                    {item.parent_id ? (
                      <span className="font-mono text-xs">{item.parent_id}</span>
                    ) : (
                      <Empty />
                    )}
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
