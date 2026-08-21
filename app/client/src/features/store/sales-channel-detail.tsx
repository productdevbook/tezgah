import { Link } from "@tanstack/react-router"

import { salesChannel } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function SalesChannelDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["sales-channels"],
    "/admin/sales-channels/{id}",
    salesChannel,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("empty.salesChannel"),
        description: t("general.nothingToShow"),
      }}
      back="sales channels"
      title={(item) => item.name}
      actions={(item) => (
        <>
          <Badge variant={item.is_disabled ? "outline" : "default"}>
            {item.is_disabled ? "disabled" : "selling"}
          </Badge>
          <DeleteAction
            path="/admin/sales-channels/{id}"
            params={{ id: item.id }}
            invalidateKey={["sales-channels"]}
            kind="sales channel"
            name={item.name}
          />
        </>
      )}
      main={(item) => (
        <Section
          title={t("detail.channel.title")}
          actions={
            <ActionMenu
              groups={[
                [
                  {
                    label: "Edit",
                    render: (
                      <Link
                        to="/store/sales-channels/$id/edit"
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
              label={t("field.description")}
              value={item.description}
            />
            <SectionRow
              label={t("field.state")}
              value={item.is_disabled ? "Disabled" : "Selling"}
            />
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
