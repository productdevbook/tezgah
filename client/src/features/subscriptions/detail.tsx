import { subscriptionContract } from "@/api/schemas"
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime, useDetail } from "@/lib/detail"

export function SubscriptionDetail({ id }: { id: string }) {
  const result = useDetail(
    ["subscriptions"],
    "/admin/subscriptions/{id}",
    subscriptionContract,
    id
  )

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No subscription", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader
              back="subscriptions"
              title={`Subscription — cycle ${item.cycle}`}
              subtitle={item.customer_id}
            >
              <Badge variant={item.ended_at ? "outline" : "default"}>{item.status}</Badge>
              {item.cancel_at_period_end ? (
                <Badge variant="outline">ends this period</Badge>
              ) : null}
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Customer">
                    <span className="font-mono text-xs">{item.customer_id}</span>
                  </DetailField>
                  <DetailField label="Selling plan">
                    <span className="font-mono text-xs">{item.selling_plan_id}</span>
                  </DetailField>
                  <DetailField label="Status">{item.status}</DetailField>
                  <DetailField label="Currency">
                    <span className="font-mono text-xs uppercase">{item.currency_code}</span>
                  </DetailField>
                  <DetailField label="Cycle">{item.cycle}</DetailField>
                  <DetailField label="Cancel at period end">
                    {item.cancel_at_period_end ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Dunning attempts">
                    {item.dunning_attempts > 0 ? (
                      <span className="flex items-center gap-1.5">
                        <Badge variant="destructive">{item.dunning_attempts}</Badge>
                        {/* Above zero means a charge failed and is being
                            retried — a different thing from a cancelled
                            contract, which `status`/`ended_at` say instead. */}
                        <span className="text-xs text-muted-foreground">
                          retrying a failed charge, not cancelled
                        </span>
                      </span>
                    ) : (
                      <Empty />
                    )}
                  </DetailField>
                  <DetailField label="Next billing">
                    {dateTime(item.next_billing_at)}
                  </DetailField>
                  <DetailField label="Current period">
                    {dateTime(item.current_period_start)} – {dateTime(item.current_period_end)}
                  </DetailField>
                  <DetailField label="Ended">
                    {item.ended_at ? dateTime(item.ended_at) : <Empty />}
                  </DetailField>
                </FieldGrid>
              </CardContent>
            </Card>
            <Card>
              <CardContent>
                <h2 className="mb-3 text-sm font-medium">Lines</h2>
                {item.lines.length === 0 ? (
                  <p className="text-sm text-muted-foreground">Nothing on this contract.</p>
                ) : (
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Title</TableHead>
                        <TableHead>Variant</TableHead>
                        <TableHead className="text-right">Quantity</TableHead>
                        <TableHead className="text-right">Unit price</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {item.lines.map((line) => (
                        <TableRow key={line.variant_id}>
                          <TableCell>{line.title ?? <Empty />}</TableCell>
                          <TableCell className="font-mono text-xs">
                            {line.variant_id}
                          </TableCell>
                          <TableCell className="text-right">{line.quantity}</TableCell>
                          <TableCell className="text-right font-mono text-xs">
                            {line.unit_price} {line.currency_code.toUpperCase()}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                )}
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
  )
}
