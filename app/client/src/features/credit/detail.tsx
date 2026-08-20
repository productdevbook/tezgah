import { giftCard } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Movements } from "@/features/credit/movements"
import { dateTime, useDetail } from "@/lib/detail"

export function GiftCardDetail({ id }: { id: string }) {
  const result = useDetail(
    ["gift-cards"],
    "/admin/gift-cards/{id}",
    giftCard,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No gift card", description: "Nothing to show." }}
      back="credit"
      title={(item) => `${item.balance} ${item.currency_code.toUpperCase()}`}
      actions={(item) => (
        <Badge variant={item.disabled_at ? "outline" : "default"}>
          {item.disabled_at ? "disabled" : "active"}
        </Badge>
      )}
      main={(item) => (
        <>
          <Section title="The balance">
            <SectionRows>
              <SectionRow
                label="Now"
                value={
                  <Mono>
                    {item.balance} {item.currency_code.toUpperCase()}
                  </Mono>
                }
              />
              <SectionRow
                label="Issued with"
                value={
                  <Mono>
                    {item.initial_balance} {item.currency_code.toUpperCase()}
                  </Mono>
                }
              />
              <SectionRow
                label="Expires"
                value={item.expires_at ? dateTime(item.expires_at) : null}
              />
              <SectionRow
                label="Disabled"
                value={item.disabled_at ? dateTime(item.disabled_at) : null}
              />
            </SectionRows>
          </Section>

          <Movements path="/admin/gift-cards/{id}/transactions" id={item.id} />
        </>
      )}
      side={(item) => (
        <>
          <Section title="Where it came from">
            <SectionRows>
              <SectionRow
                label="Customer"
                value={
                  item.customer_id ? <Mono>{item.customer_id}</Mono> : null
                }
              />
              <SectionRow
                label="Issued on order"
                value={
                  item.issued_order_id ? (
                    <Mono>{item.issued_order_id}</Mono>
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
