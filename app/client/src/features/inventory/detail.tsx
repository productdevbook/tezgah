import { inventoryItem } from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Levels } from "@/features/inventory/levels"
import { dateTime, useDetail } from "@/lib/detail"

export function InventoryItemDetail({ id }: { id: string }) {
  const result = useDetail(
    ["inventory-items"],
    "/admin/inventory-items/{id}",
    inventoryItem,
    id
  )

  return (
    <DetailPage
      query={result}
      empty={{ title: "No inventory item", description: "Nothing to show." }}
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
          <Section title="The item">
            <SectionRows>
              <SectionRow label="Title" value={item.title} />
              <SectionRow
                label="SKU"
                value={item.sku ? <Mono>{item.sku}</Mono> : null}
              />
              <SectionRow
                label="Ships"
                value={
                  item.requires_shipping ? "Shipped" : "Digital, no shipping"
                }
              />
            </SectionRows>
          </Section>
          <Levels itemId={item.id} />
        </>
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
