import { inventoryItem } from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Levels } from "@/features/inventory/levels"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

export function InventoryItemDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(
    ["inventory-items"],
    "/admin/inventory-items/{id}",
    inventoryItem,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("detail.inventory.empty"),
        description: t("detail.nothingToShow"),
      }}
      back="inventory"
      title={(item) => item.title ?? "Untitled item"}
      subtitle={(item) => item.sku ?? undefined}
      actions={(item) => (
        <>
          <Badge variant={item.requires_shipping ? "default" : "outline"}>
            {item.requires_shipping ? "shipped" : "digital"}
          </Badge>
          {/* No PATCH for an inventory item past creation — the only write
              past this is the stock a location holds of it, already bound at
              POST .../{id}/location-levels — so this screen offers delete
              without an edit to go with it. */}
          <DeleteAction
            path="/admin/inventory-items/{id}"
            params={{ id: item.id }}
            invalidateKey={["inventory-items"]}
            kind="inventory item"
            name={item.title ?? item.sku ?? "this item"}
          />
        </>
      )}
      main={(item) => (
        <>
          <Section title={t("detail.inventory.title")}>
            <SectionRows>
              <SectionRow label={t("field.title")} value={item.title} />
              <SectionRow
                label={t("field.sku")}
                value={item.sku ? <Mono>{item.sku}</Mono> : null}
              />
              <SectionRow
                label={t("field.ships")}
                value={
                  item.requires_shipping
                    ? t("value.shipped")
                    : t("value.digital")
                }
              />
            </SectionRows>
          </Section>
          <Levels itemId={item.id} />
        </>
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
