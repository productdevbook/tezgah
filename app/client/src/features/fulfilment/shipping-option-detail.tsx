import { shippingOption } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function ShippingOptionDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["shipping-options"],
    "/admin/shipping-options/{id}",
    shippingOption,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("empty.shippingOption"),
        description: t("general.nothingToShow"),
      }}
      back="shipping options"
      title={(item) => item.name}
      actions={(item) => (
        <>
          <Badge variant="outline">{item.price_type}</Badge>
          {item.is_return ? <Badge>return</Badge> : null}
        </>
      )}
      main={(item) => (
        <Section title={t("detail.shippingOption.title")}>
          <SectionRows>
            <SectionRow label={t("field.name")} value={item.name} />
            <SectionRow label={t("field.priced")} value={item.price_type} />
            <SectionRow
              label={t("field.offeredToShoppers")}
              value={item.enabled_in_store ? "Yes" : "No"}
            />
            <SectionRow
              label={t("field.forReturns")}
              value={item.is_return ? "Yes" : "No"}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title={t("detail.shippingOption.where")}
            description={t("detail.shippingOption.whereWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.serviceZone")}
                value={<Mono>{item.service_zone_id}</Mono>}
              />
              <SectionRow
                label={t("field.shippingProfile")}
                value={
                  item.shipping_profile_id ? (
                    <Mono>{item.shipping_profile_id}</Mono>
                  ) : null
                }
              />
              <SectionRow
                label={t("field.optionType")}
                value={
                  item.shipping_option_type_id ? (
                    <Mono>{item.shipping_option_type_id}</Mono>
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
