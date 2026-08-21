import { useQuery } from "@tanstack/react-query"

import { get } from "@/api/client"
import { GetAdminCustomersByIdStoreCreditResponse } from "@/api/generated/zod/credit/credit"
import { Movements } from "@/features/credit/movements"
import {
  GetAdminCustomersByIdTaxExemptionsResponse,
  GetAdminCustomersByIdTaxIdsResponse,
} from "@/api/generated/zod/tax/tax"
import { Empty, Mono } from "@/components/detail-fields"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { ApiError } from "@/api/errors"
import { dateTime } from "@/lib/detail"
import { useT } from "@/panel/i18n"

/**
 * What hangs off a customer.
 *
 * Three routes the server has bound since it was written and no screen asked
 * for: the balance they can spend, the tax numbers they have given, and the
 * certificates that exempt them. Each is a separate request because each is
 * a separate route — the customer view carries none of it.
 */
export function StoreCredit({ customerId }: { customerId: string }) {
  const t = useT()
  const result = useQuery({
    queryKey: ["customer-store-credit", customerId],
    queryFn: ({ signal }) =>
      get("/admin/customers/{id}/store-credit", {
        signal,
        schema: GetAdminCustomersByIdStoreCreditResponse,
        params: { id: customerId },
      }),
    // A customer with no balance has no row, and the route says so with a
    // 404. That is an answer, not a failure, so it is not retried.
    retry: false,
  })

  const missing =
    result.error instanceof ApiError && result.error.status === 404

  return (
    <Section
      title={t("attached.storeCredit")}
      description={t("attached.storeCreditWhy")}
    >
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : missing || !result.data ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          No balance. One appears the first time the shop puts money on it.
        </p>
      ) : (
        <>
          <SectionRows>
            <SectionRow
              label={t("field.balance")}
              value={
                <span className="font-mono">
                  {result.data.balance}{" "}
                  {result.data.currency_code.toUpperCase()}
                </span>
              }
            />
            <SectionRow
              label={t("field.usable")}
              value={
                result.data.disabled_at
                  ? `No — disabled ${dateTime(result.data.disabled_at)}`
                  : "Yes"
              }
            />
          </SectionRows>
          <Movements
            path="/admin/store-credits/{id}/transactions"
            id={result.data.id}
            bare
          />
        </>
      )}
    </Section>
  )
}

export function TaxIds({ customerId }: { customerId: string }) {
  const t = useT()
  const result = useQuery({
    queryKey: ["customer-tax-ids", customerId],
    queryFn: ({ signal }) =>
      get("/admin/customers/{id}/tax-ids", {
        signal,
        schema: GetAdminCustomersByIdTaxIdsResponse,
        params: { id: customerId },
      }),
  })

  const rows = result.data ?? []

  return (
    <Section
      title={t("attached.taxNumbers")}
      description={t("attached.taxNumbersWhy")}
    >
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">None given.</p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Number</TableHead>
              <TableHead>Kind</TableHead>
              <TableHead>Country</TableHead>
              <TableHead>Checked</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell>
                  <Mono>{row.tax_id}</Mono>
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {row.tax_id_type}
                </TableCell>
                <TableCell className="uppercase">
                  {row.tax_id_country}
                </TableCell>
                <TableCell>
                  {row.validated_at ? (
                    <Badge variant="default">
                      {dateTime(row.validated_at)}
                    </Badge>
                  ) : (
                    <Badge variant="outline">unchecked</Badge>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  )
}

export function TaxExemptions({ customerId }: { customerId: string }) {
  const t = useT()
  const result = useQuery({
    queryKey: ["customer-tax-exemptions", customerId],
    queryFn: ({ signal }) =>
      get("/admin/customers/{id}/tax-exemptions", {
        signal,
        schema: GetAdminCustomersByIdTaxExemptionsResponse,
        params: { id: customerId },
      }),
  })

  const rows = result.data ?? []

  return (
    <Section
      title={t("attached.exemptions")}
      description={t("attached.exemptionsWhy")}
    >
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          None on file. This customer is charged tax like anybody else.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Where</TableHead>
              <TableHead>Kind</TableHead>
              <TableHead>Good for</TableHead>
              <TableHead>Certificate</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell className="uppercase">
                  {row.country_code}
                  {row.province_code ? ` / ${row.province_code}` : ""}
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {row.kind}
                </TableCell>
                <TableCell className="text-xs">
                  {dateTime(row.valid_from)}
                  {row.valid_until
                    ? ` → ${dateTime(row.valid_until)}`
                    : " → no end"}
                </TableCell>
                <TableCell>
                  {row.certificate_reference ? (
                    <Mono>{row.certificate_reference}</Mono>
                  ) : (
                    <Empty />
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  )
}
