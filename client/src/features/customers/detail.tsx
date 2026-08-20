import { Link } from "@tanstack/react-router"

import { customer, type Customer } from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { DetailField, FieldGrid, Metadata, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { dateTime, useDetail } from "@/lib/detail"

function name(row: Customer): string | null {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? null)
}

export function CustomerDetail({ id }: { id: string }) {
  const result = useDetail(["customers"], "/admin/customers/{id}", customer, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No customer", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader
              back="customers"
              title={name(item) ?? "Unnamed customer"}
              subtitle={item.email ?? undefined}
            >
              <Badge variant={item.has_account ? "default" : "outline"}>
                {item.has_account ? "registered" : "guest"}
              </Badge>
              {/* Erased on request; the orders stay, the person does not. */}
              {item.anonymised ? <Badge variant="outline">erased</Badge> : null}
              <Button
                variant="outline"
                size="sm"
                nativeButton={false}
                render={<Link to="/customers/$id/edit" params={{ id: item.id }} />}
              >
                Edit
              </Button>
              <DeleteAction
                path="/admin/customers/{id}"
                params={{ id: item.id }}
                invalidateKey={["customers"]}
                kind="customer"
                name={name(item) ?? item.email ?? "this customer"}
              />
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Email">{item.email ?? <Empty />}</DetailField>
                  <DetailField label="First name">
                    {item.first_name ?? <Empty />}
                  </DetailField>
                  <DetailField label="Last name">{item.last_name ?? <Empty />}</DetailField>
                  <DetailField label="Phone">{item.phone ?? <Empty />}</DetailField>
                  <DetailField label="Company">
                    {item.company_name ?? <Empty />}
                  </DetailField>
                  <DetailField label="Account">
                    {item.has_account ? "registered" : "guest"}
                  </DetailField>
                  <DetailField label="Anonymised">
                    {item.anonymised ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Updated">{dateTime(item.updated_at)}</DetailField>
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
