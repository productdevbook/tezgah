import { Link } from "@tanstack/react-router"

import { customer, type Customer } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Mono, MetadataSection } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { dateTime, useDetail } from "@/lib/detail"

function name(row: Customer): string | null {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? null)
}

export function CustomerDetail({ id }: { id: string }) {
  const result = useDetail(["customers"], "/admin/customers/{id}", customer, id)

  return (
    <DetailPage
      query={result}
      empty={{ title: "No customer", description: "Nothing to show." }}
      back="customers"
      title={(item) => name(item) ?? "Unnamed customer"}
      subtitle={(item) => item.email ?? undefined}
      actions={(item) => (
        <>
          <Badge variant={item.has_account ? "default" : "outline"}>
            {item.has_account ? "registered" : "guest"}
          </Badge>
          {/* Erased on request; the orders stay, the person does not. */}
          {item.anonymised ? <Badge variant="outline">erased</Badge> : null}
          <DeleteAction
            path="/admin/customers/{id}"
            params={{ id: item.id }}
            invalidateKey={["customers"]}
            kind="customer"
            name={name(item) ?? item.email ?? "this customer"}
          />
        </>
      )}
      main={(item) => (
        <Section
          title="Who they are"
          actions={
            <ActionMenu
              groups={[
                [
                  {
                    label: "Edit",
                    render: (
                      <Link to="/customers/$id/edit" params={{ id: item.id }} />
                    ),
                  },
                ],
              ]}
            />
          }
        >
          <SectionRows>
            <SectionRow label="Email" value={item.email} />
            <SectionRow label="First name" value={item.first_name} />
            <SectionRow label="Last name" value={item.last_name} />
            <SectionRow label="Phone" value={item.phone} />
            <SectionRow label="Company" value={item.company_name} />
          </SectionRows>
        </Section>
      )}
      side={(item) => (
        <>
          <Section title="Account">
            <SectionRows>
              <SectionRow
                label="Account"
                value={item.has_account ? "Registered" : "Guest"}
              />
              <SectionRow
                label="Erased"
                value={item.anonymised ? "Yes" : "No"}
              />
            </SectionRows>
          </Section>

          <MetadataSection value={item.metadata} />

          <Section title="Details">
            <SectionRows>
              <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
              <SectionRow label="Created" value={dateTime(item.created_at)} />
              <SectionRow label="Updated" value={dateTime(item.updated_at)} />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}
