import { shippingProfile } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function ShippingProfileDetail({ id }: { id: string }) {
  const t = useT()
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
          title={t("detail.shippingProfile.title")}
          description={t("detail.shippingProfile.why")}
        >
          <SectionRows>
            <SectionRow label={t("field.name")} value={item.name} />
            <SectionRow label={t("field.kind")} value={item.kind} />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <Section title={t("general.details")}>
          <SectionRows>
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
