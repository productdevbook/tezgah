import { taxRate } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { TaxRateRules } from "@/features/tax/rules"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function TaxRateDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(["tax-rates"], "/admin/tax-rates/{id}", taxRate, id)

  return (
    <DetailPage
      query={result}
      empty={{ title: "No tax rate", description: "Nothing to show." }}
      back="tax rates"
      title={(item) => item.name}
      actions={(item) => (
        <>
          {item.is_default ? <Badge>default</Badge> : null}
          {item.is_combinable ? (
            <Badge variant="outline">combinable</Badge>
          ) : null}
        </>
      )}
      main={(item) => (
        <>
          <Section
            title={t("detail.taxRate.title")}
            description={t("detail.taxRate.why")}
          >
            <SectionRows>
              <SectionRow label={t("field.name")} value={item.name} />
              <SectionRow
                label={t("field.rate")}
                value={<Mono>{item.rate}</Mono>}
              />
              <SectionRow label={t("field.code")} value={item.code} />
              <SectionRow
                label={t("field.defaultForRegion")}
                value={item.is_default ? "Yes" : "No"}
              />
              <SectionRow
                label={t("field.combinable")}
                value={item.is_combinable ? "Yes" : "No"}
              />
            </SectionRows>
          </Section>

          <TaxRateRules rateId={item.id} />
        </>
      )}
      side={(item) => (
        <Section title={t("general.details")}>
          <SectionRows>
            <SectionRow
              label={t("field.taxRegion")}
              value={<Mono>{item.tax_region_id}</Mono>}
            />
            <SectionRow label={t("field.id")} value={<Mono>{item.id}</Mono>} />
            <SectionRow
              label={t("field.created")}
              value={dateTime(item.created_at)}
            />
          </SectionRows>
        </Section>
      )}
    />
  )
}
