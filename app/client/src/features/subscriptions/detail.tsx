import { subscriptionContract } from "@/api/schemas"
import { SubscriptionActions } from "@/features/subscriptions/actions"
import { Empty, Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import {
  Section,
  SectionBody,
  SectionRow,
  SectionRows,
} from "@/components/section"
import { Badge } from "@/components/ui/badge"
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
    <DetailPage
      query={result}
      empty={{ title: "No subscription", description: "Nothing to show." }}
      back="subscriptions"
      title={(item) => `Subscription — cycle ${item.cycle}`}
      subtitle={(item) => item.customer_id}
      actions={(item) => (
        <>
          <Badge variant={item.ended_at ? "outline" : "default"}>
            {item.status}
          </Badge>
          {item.cancel_at_period_end ? (
            <Badge variant="outline">ends this period</Badge>
          ) : null}
          <SubscriptionActions item={item} />
        </>
      )}
      main={(item) => (
        <>
          <Section title="What is being billed">
            {item.lines.length === 0 ? (
              <SectionBody>
                <p className="text-sm text-muted-foreground">
                  Nothing on this contract.
                </p>
              </SectionBody>
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
                      <TableCell>
                        <Mono>{line.variant_id}</Mono>
                      </TableCell>
                      <TableCell className="text-right">
                        {line.quantity}
                      </TableCell>
                      <TableCell className="text-right">
                        <Mono>
                          {line.unit_price} {line.currency_code.toUpperCase()}
                        </Mono>
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            )}
          </Section>

          <Section title="The cycle">
            <SectionRows>
              <SectionRow label="Cycle" value={item.cycle} />
              <SectionRow
                label="Next billing"
                value={dateTime(item.next_billing_at)}
              />
              <SectionRow
                label="Current period"
                value={`${dateTime(item.current_period_start)} – ${dateTime(
                  item.current_period_end
                )}`}
              />
              <SectionRow
                label="Ends this period"
                value={item.cancel_at_period_end ? "Yes" : "No"}
              />
              <SectionRow
                label="Ended"
                value={item.ended_at ? dateTime(item.ended_at) : null}
              />
            </SectionRows>
          </Section>
        </>
      )}
      side={(item) => (
        <>
          <Section
            title="Collection"
            description="Above zero means a charge failed and is being retried — a different thing from a cancelled contract, which the status says instead."
          >
            <SectionRows>
              <SectionRow label="Status" value={item.status} />
              <SectionRow
                label="Dunning attempts"
                value={
                  item.dunning_attempts > 0 ? (
                    <Badge variant="destructive">{item.dunning_attempts}</Badge>
                  ) : null
                }
              />
            </SectionRows>
          </Section>

          <Section title="Who and what">
            <SectionRows>
              <SectionRow
                label="Customer"
                value={<Mono>{item.customer_id}</Mono>}
              />
              <SectionRow
                label="Selling plan"
                value={<Mono>{item.selling_plan_id}</Mono>}
              />
              <SectionRow
                label="Currency"
                value={<Mono>{item.currency_code.toUpperCase()}</Mono>}
              />
            </SectionRows>
          </Section>

          <Section title="Details">
            <SectionRows>
              <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}
