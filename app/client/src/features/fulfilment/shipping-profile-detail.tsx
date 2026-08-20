import { shippingProfile } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function ShippingProfileDetail({ id }: { id: string }) {
  const result = useDetail(
    ["shipping-profiles"],
    "/admin/shipping-profiles/{id}",
    shippingProfile,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No shipping profile", description: "Nothing to show." }}
      back="shipping profiles"
      title={(item) => item.name}
      actions={(item) => <Badge variant="outline">{item.kind}</Badge>}
      main={(item) => (
        <Section
          title="The profile"
          description="What a shipping option is allowed to carry — goods that travel together, and goods that cannot."
        >
          <SectionRows>
            <SectionRow label="Name" value={item.name} />
            <SectionRow label="Kind" value={item.kind} />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <Section title="Details">
          <SectionRows>
            <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
            <SectionRow label="Created" value={dateTime(item.created_at)} />
          </SectionRows>
        </Section>
      )}
    />
  )
}
