import { Link } from "@tanstack/react-router"

import { promotion } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Mono, MetadataSection } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

export function PromotionDetail({ id }: { id: string }) {
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
          title="The promotion"
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
            <SectionRow label="Code" value={<Mono>{item.code}</Mono>} />
            <SectionRow label="Kind" value={item.kind} />
            <SectionRow label="Status" value={item.status} />
            <SectionRow
              label="Applied"
              value={item.is_automatic ? "Automatically" : "By code"}
            />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section
            title="How much is left"
            description="Claimed at checkout rather than counted at payment, so this is what is spoken for."
          >
            <SectionRows>
              <SectionRow
                label="Used"
                value={
                  item.usage_limit === null
                    ? `${item.used}`
                    : `${item.used} / ${item.usage_limit}`
                }
              />
              <SectionRow
                label="Per customer"
                value={item.customer_usage_limit}
              />
              <SectionRow
                label="Campaign"
                value={
                  item.campaign_id ? <Mono>{item.campaign_id}</Mono> : null
                }
              />
            </SectionRows>
          </Section>

          <MetadataSection value={item.metadata} />

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
