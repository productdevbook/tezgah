import { taxRate } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { TaxRateRules } from "@/features/tax/rules"
import { dateTime, useDetail } from "@/lib/detail"

export function TaxRateDetail({ id }: { id: string }) {
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
            title="The rate"
            description="One region has exactly one default rate; a combinable rate stacks on top of whichever applies."
          >
            <SectionRows>
              <SectionRow label="Name" value={item.name} />
              <SectionRow label="Rate" value={<Mono>{item.rate}</Mono>} />
              <SectionRow label="Code" value={item.code} />
              <SectionRow
                label="Default for its region"
                value={item.is_default ? "Yes" : "No"}
              />
              <SectionRow
                label="Combinable"
                value={item.is_combinable ? "Yes" : "No"}
              />
            </SectionRows>
          </Section>

          <TaxRateRules rateId={item.id} />
        </>
      )}
      side={(item) => (
        <Section title="Details">
          <SectionRows>
            <SectionRow
              label="Tax region"
              value={<Mono>{item.tax_region_id}</Mono>}
            />
            <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
            <SectionRow label="Created" value={dateTime(item.created_at)} />
          </SectionRows>
        </Section>
      )}
    />
  )
}
