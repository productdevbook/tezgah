import { taxRegion } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { dateTime, useDetail } from "@/lib/detail"

export function TaxRegionDetail({ id }: { id: string }) {
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
          title="Where it applies"
          description="Tax regions nest: a province's rates sit under its country's."
        >
          <SectionRows>
            <SectionRow
              label="Country"
              value={<Mono>{item.country_code.toUpperCase()}</Mono>}
            />
            <SectionRow
              label="Province"
              value={
                item.province_code ? (
                  <Mono>{item.province_code.toUpperCase()}</Mono>
                ) : null
              }
            />
            <SectionRow
              label="Parent region"
              value={item.parent_id ? <Mono>{item.parent_id}</Mono> : null}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title="Who works the tax out">
            <SectionRows>
              <SectionRow label="Provider" value={item.provider} />
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
