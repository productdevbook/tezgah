import { taxRegion } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function TaxRegionDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["tax-regions"],
    "/admin/tax-regions/{id}",
    taxRegion,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No tax region", description: "Nothing to show." }}
      back="tax regions"
      title={(item) => item.country_code.toUpperCase()}
      main={(item) => (
        <Section
          title={t("detail.shippingOption.where")}
          description={t("detail.taxRegion.whereWhy")}
        >
          <SectionRows>
            <SectionRow
              label={t("field.country")}
              value={<Mono>{item.country_code.toUpperCase()}</Mono>}
            />
            <SectionRow
              label={t("field.province")}
              value={
                item.province_code ? (
                  <Mono>{item.province_code.toUpperCase()}</Mono>
                ) : null
              }
            />
            <SectionRow
              label={t("field.parentRegion")}
              value={item.parent_id ? <Mono>{item.parent_id}</Mono> : null}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.taxRegion.who")}>
            <SectionRows>
              <SectionRow label={t("field.provider")} value={item.provider} />
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
