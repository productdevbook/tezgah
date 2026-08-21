import { payment } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function PaymentDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(["payments"], "/admin/payments/{id}", payment, id)

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("empty.payment"),
        description: t("general.nothingToShow"),
      }}
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
          title={t("detail.payment.what")}
          description={t("detail.payment.whatWhy")}
        >
          <SectionRows>
            <SectionRow
              label={t("field.amount")}
              value={
                <Mono>
                  {item.amount.amount} {item.amount.currency.toUpperCase()}
                </Mono>
              }
            />
            <SectionRow
              label={t("field.captured")}
              value={item.captured_at ? dateTime(item.captured_at) : null}
            />
            <SectionRow
              label={t("field.canceled")}
              value={item.canceled_at ? dateTime(item.canceled_at) : null}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.payment.where")}>
            <SectionRows>
              <SectionRow
                label={t("field.collection")}
                value={<Mono>{item.payment_collection_id}</Mono>}
              />
              <SectionRow
                label={t("field.session")}
                value={
                  item.payment_session_id ? (
                    <Mono>{item.payment_session_id}</Mono>
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
