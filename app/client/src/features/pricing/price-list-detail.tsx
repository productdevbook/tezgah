import { priceList } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function PriceListDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["price-lists"],
    "/admin/price-lists/{id}",
    priceList,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No price list", description: "Nothing to show." }}
      back="price lists"
      title={(item) => item.title}
      actions={(item) => (
        <>
          <Badge variant="outline">{item.kind}</Badge>
          <Badge>{item.status}</Badge>
        </>
      )}
      main={(item) => (
        <Section
          title={t("detail.priceList.title")}
          description={t("detail.priceList.why")}
        >
          <SectionRows>
            <SectionRow label={t("field.title")} value={item.title} />
            <SectionRow
              label={t("field.description")}
              value={item.description}
            />
            <SectionRow label={t("field.kind")} value={item.kind} />
            <SectionRow label={t("field.status")} value={item.status} />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.priceList.when")}>
            <SectionRows>
              <SectionRow
                label={t("field.starts")}
                value={item.starts_at ? dateTime(item.starts_at) : null}
              />
              <SectionRow
                label={t("field.ends")}
                value={item.ends_at ? dateTime(item.ends_at) : null}
              />
              <SectionRow label={t("field.rules")} value={item.rules_count} />
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
