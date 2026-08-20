import { shippingOption } from "@/api/schemas"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function ShippingOptionDetail({ id }: { id: string }) {
  const result = useDetail(
    ["shipping-options"],
    "/admin/shipping-options/{id}",
    shippingOption,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No shipping option", description: "Nothing to show." }}
      back="shipping options"
      title={(item) => item.name}
      actions={(item) => (
        <>
          <Badge variant="outline">{item.price_type}</Badge>
          {item.is_return ? <Badge>return</Badge> : null}
        </>
      )}
      main={(item) => (
        <Section title="The option">
          <SectionRows>
            <SectionRow label="Name" value={item.name} />
            <SectionRow label="Priced" value={item.price_type} />
            <SectionRow
              label="Offered to shoppers"
              value={item.enabled_in_store ? "Yes" : "No"}
            />
            <SectionRow
              label="For returns"
              value={item.is_return ? "Yes" : "No"}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title="Where it applies"
            description="A service zone is the set of addresses this option is offered at; the profile decides which goods it can carry."
          >
            <SectionRows>
              <SectionRow
                label="Service zone"
                value={<Mono>{item.service_zone_id}</Mono>}
              />
              <SectionRow
                label="Shipping profile"
                value={
                  item.shipping_profile_id ? (
                    <Mono>{item.shipping_profile_id}</Mono>
                  ) : null
                }
              />
              <SectionRow
                label="Option type"
                value={
                  item.shipping_option_type_id ? (
                    <Mono>{item.shipping_option_type_id}</Mono>
                  ) : null
                }
              />
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
