import { giftCard } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Movements } from "@/features/credit/movements"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function GiftCardDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["gift-cards"],
    "/admin/gift-cards/{id}",
    giftCard,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("empty.giftCard"),
        description: t("general.nothingToShow"),
      }}
      back="credit"
      title={(item) => `${item.balance} ${item.currency_code.toUpperCase()}`}
      actions={(item) => (
        <Badge variant={item.disabled_at ? "outline" : "default"}>
          {item.disabled_at ? "disabled" : "active"}
        </Badge>
      )}
      main={(item) => (
        <>
          <Section title={t("detail.giftCard.balance")}>
            <SectionRows>
              <SectionRow
                label={t("field.now")}
                value={
                  <Mono>
                    {item.balance} {item.currency_code.toUpperCase()}
                  </Mono>
                }
              />
              <SectionRow
                label={t("field.issuedWith")}
                value={
                  <Mono>
                    {item.initial_balance} {item.currency_code.toUpperCase()}
                  </Mono>
                }
              />
              <SectionRow
                label={t("field.expires")}
                value={item.expires_at ? dateTime(item.expires_at) : null}
              />
              <SectionRow
                label={t("field.disabled")}
                value={item.disabled_at ? dateTime(item.disabled_at) : null}
              />
            </SectionRows>
          </Section>

          <Movements path="/admin/gift-cards/{id}/transactions" id={item.id} />
        </>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.giftCard.origin")}>
            <SectionRows>
              <SectionRow
                label={t("field.customer")}
                value={
                  item.customer_id ? <Mono>{item.customer_id}</Mono> : null
                }
              />
              <SectionRow
                label={t("field.issuedOnOrder")}
                value={
                  item.issued_order_id ? (
                    <Mono>{item.issued_order_id}</Mono>
                  ) : null
                }
              />
            </SectionRows>
          </Section>

          <Section title={t("general.details")}>
            <SectionRows>
              <SectionRow
                label={t("field.id")}
                value={<Mono>{item.id}</Mono>}
              />
              <SectionRow
                label={t("field.created")}
                value={dateTime(item.created_at)}
              />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}
