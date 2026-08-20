import { Link } from "@tanstack/react-router"

import { region } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function RegionDetail({ id }: { id: string }) {
  const result = useDetail(["regions"], "/admin/regions/{id}", region, id)

  return (
    <DetailPage
      query={result}
      empty={{ title: "No region", description: "Nothing to show." }}
      back="regions"
      title={(item) => item.name}
      actions={(item) => (
        <>
          <Badge variant="outline">
            {item.is_tax_inclusive ? "tax inclusive" : "tax exclusive"}
          </Badge>
          {item.has_automatic_taxes ? <Badge>automatic taxes</Badge> : null}
        </>
      )}
      main={(item) => (
        <Section
          title="The region"
          actions={
            <ActionMenu
              groups={[
                [
                  {
                    label: "Edit",
                    render: (
                      <Link
                        to="/store/regions/$id/edit"
                        params={{ id: item.id }}
                      />
                    ),
                  },
                ],
              ]}
            />
          }
        >
          <SectionRows>
            <SectionRow label="Name" value={item.name} />
            <SectionRow
              label="Currency"
              value={<Mono>{item.currency_code.toUpperCase()}</Mono>}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title="Tax"
            description="Whether a shown price already contains tax, and who works it out."
          >
            <SectionRows>
              <SectionRow
                label="Prices"
                value={
                  item.is_tax_inclusive
                    ? "Include tax"
                    : "Have tax added at the till"
                }
              />
              <SectionRow
                label="Worked out"
                value={item.has_automatic_taxes ? "Automatically" : "By hand"}
              />
            </SectionRows>
          </Section>

          <Section title="Payment providers">
            <SectionRows>
              <SectionRow
                label="Allowed"
                value={
                  item.payment_providers.length ? (
                    <Mono>{item.payment_providers.join(", ")}</Mono>
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
