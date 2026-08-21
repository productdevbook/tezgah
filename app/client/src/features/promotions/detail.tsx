import { Link } from "@tanstack/react-router"

import { promotion } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Mono, MetadataSection } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function PromotionDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["promotions"],
    "/admin/promotions/{id}",
    promotion,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No promotion", description: "Nothing to show." }}
      back="promotions"
      title={(item) => item.code}
      subtitle={(item) => item.kind}
      actions={(item) => (
        <>
          <Badge variant={item.status === "active" ? "default" : "outline"}>
            {item.status}
          </Badge>
          {item.is_automatic ? (
            <Badge variant="outline">automatic</Badge>
          ) : null}
          <DeleteAction
            path="/admin/promotions/{id}"
            params={{ id: item.id }}
            invalidateKey={["promotions"]}
            kind="promotion"
            name={item.code}
          />
        </>
      )}
      main={(item) => (
        <Section
          title={t("detail.promotion.title")}
          actions={
            <ActionMenu
              groups={[
                [
                  {
                    label: "Edit",
                    render: (
                      <Link
                        to="/promotions/$id/edit"
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
            <SectionRow
              label={t("field.code")}
              value={<Mono>{item.code}</Mono>}
            />
            <SectionRow label={t("field.kind")} value={item.kind} />
            <SectionRow label={t("field.status")} value={item.status} />
            <SectionRow
              label={t("field.applied")}
              value={item.is_automatic ? "Automatically" : "By code"}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title={t("detail.promotion.left")}
            description={t("detail.promotion.leftWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.used")}
                value={
                  item.usage_limit === null
                    ? `${item.used}`
                    : `${item.used} / ${item.usage_limit}`
                }
              />
              <SectionRow
                label={t("field.perCustomer")}
                value={item.customer_usage_limit}
              />
              <SectionRow
                label={t("field.campaign")}
                value={
                  item.campaign_id ? <Mono>{item.campaign_id}</Mono> : null
                }
              />
            </SectionRows>
          </Section>

          <MetadataSection value={item.metadata} />

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
