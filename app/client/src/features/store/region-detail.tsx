import { Link } from "@tanstack/react-router"

import { region } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function RegionDetail({ id }: { id: string }) {
  const t = useT()
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
          title={t("detail.region.title")}
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
            <SectionRow label={t("field.name")} value={item.name} />
            <SectionRow
              label={t("field.currency")}
              value={<Mono>{item.currency_code.toUpperCase()}</Mono>}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title={t("field.tax")}
            description={t("detail.region.taxWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.prices")}
                value={
                  item.is_tax_inclusive
                    ? "Include tax"
                    : "Have tax added at the till"
                }
              />
              <SectionRow
                label={t("field.workedOut")}
                value={item.has_automatic_taxes ? "Automatically" : "By hand"}
              />
            </SectionRows>
          </Section>

          <Section title={t("detail.region.providers")}>
            <SectionRows>
              <SectionRow
                label={t("field.allowed")}
                value={
                  item.payment_providers.length ? (
                    <Mono>{item.payment_providers.join(", ")}</Mono>
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
