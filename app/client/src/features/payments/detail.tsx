import { payment } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function PaymentDetail({ id }: { id: string }) {
  const result = useDetail(["payments"], "/admin/payments/{id}", payment, id)

  return (
    <DetailPage
      query={result}
      empty={{ title: "No payment", description: "Nothing to show." }}
      back="payments"
      title={(item) =>
        `${item.amount.amount} ${item.amount.currency.toUpperCase()}`
      }
      actions={(item) => (
        <>
          {item.captured_at ? <Badge>captured</Badge> : null}
          {item.canceled_at ? <Badge variant="outline">canceled</Badge> : null}
        </>
      )}
      main={(item) => (
        <Section
          title="What happened to the money"
          description="Authorising and capturing are separate acts here, so a payment that exists is not yet a payment that was taken."
        >
          <SectionRows>
            <SectionRow
              label="Amount"
              value={
                <Mono>
                  {item.amount.amount} {item.amount.currency.toUpperCase()}
                </Mono>
              }
            />
            <SectionRow
              label="Captured"
              value={item.captured_at ? dateTime(item.captured_at) : null}
            />
            <SectionRow
              label="Canceled"
              value={item.canceled_at ? dateTime(item.canceled_at) : null}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title="Where it sits">
            <SectionRows>
              <SectionRow
                label="Collection"
                value={<Mono>{item.payment_collection_id}</Mono>}
              />
              <SectionRow
                label="Session"
                value={
                  item.payment_session_id ? (
                    <Mono>{item.payment_session_id}</Mono>
                  ) : null
                }
              />
            </SectionRows>
          </Section>

          <Section title="Details">
            <SectionRows>
              <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
              <SectionRow label="Created" value={dateTime(item.created_at)} />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}
