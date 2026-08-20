import { priceList } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function PriceListDetail({ id }: { id: string }) {
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
          title="The list"
          description="A sale list marks the price down and says so; an override replaces it silently."
        >
          <SectionRows>
            <SectionRow label="Title" value={item.title} />
            <SectionRow label="Description" value={item.description} />
            <SectionRow label="Kind" value={item.kind} />
            <SectionRow label="Status" value={item.status} />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title="When it applies">
            <SectionRows>
              <SectionRow
                label="Starts"
                value={item.starts_at ? dateTime(item.starts_at) : null}
              />
              <SectionRow
                label="Ends"
                value={item.ends_at ? dateTime(item.ends_at) : null}
              />
              <SectionRow label="Rules" value={item.rules_count} />
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
