import { Link } from "@tanstack/react-router"

import { customer, type Customer } from "@/api/schemas"
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Mono, MetadataSection } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import {
  StoreCredit,
  TaxExemptions,
  TaxIds,
} from "@/features/customers/attached"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

function name(row: Customer): string | null {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? null)
}

export function CustomerDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(["customers"], "/admin/customers/{id}", customer, id)

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("detail.customer.empty"),
        description: t("detail.nothingToShow"),
      }}
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
        <>
          <Section
            title={t("detail.customer.title")}
            actions={
              <ActionMenu
                groups={[
                  [
                    {
                      label: "Edit",
                      render: (
                        <Link
                          to="/customers/$id/edit"
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
              <SectionRow label={t("field.email")} value={item.email} />
              <SectionRow
                label={t("field.firstName")}
                value={item.first_name}
              />
              <SectionRow label={t("field.lastName")} value={item.last_name} />
              <SectionRow label={t("field.phone")} value={item.phone} />
              <SectionRow
                label={t("field.company")}
                value={item.company_name}
              />
            </SectionRows>
          </Section>

          <StoreCredit customerId={item.id} />
          <TaxIds customerId={item.id} />
          <TaxExemptions customerId={item.id} />
        </>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.customer.account")}>
            <SectionRows>
              <SectionRow
                label={t("field.account")}
                value={
                  item.has_account ? t("value.registered") : t("value.guest")
                }
              />
              <SectionRow
                label={t("field.erased")}
                value={item.anonymised ? t("value.yes") : t("value.no")}
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
              <SectionRow
                label={t("field.updated")}
                value={dateTime(item.updated_at)}
              />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}
