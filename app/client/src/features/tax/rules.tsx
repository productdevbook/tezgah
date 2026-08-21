import { useQuery } from "@tanstack/react-query"

import { get } from "@/api/client"
import { GetAdminTaxRatesByIdRulesResponse } from "@/api/generated/zod/tax/tax"
import { Mono } from "@/components/detail-fields"
import { Section } from "@/components/section"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { useT } from "@/panel/i18n"

/**
 * What narrows a rate to some of what is sold.
 *
 * A rule names a thing — a product, a product type, a shipping option — and
 * the rate applies to that rather than to everything in the region. No rules
 * means the rate applies to everything, which is the ordinary case and why
 * the empty state says so rather than looking like a failure.
 *
 * `reference_id` is shown as an id because that is all the route answers:
 * a rule does not carry the name of the thing it names, and inventing a
 * lookup per row would be this screen deciding what the API did not say.
 */
export function TaxRateRules({ rateId }: { rateId: string }) {
  const t = useT()
  const result = useQuery({
    queryKey: ["tax-rate-rules", rateId],
    queryFn: ({ signal }) =>
      get("/admin/tax-rates/{id}/rules", {
        signal,
        schema: GetAdminTaxRatesByIdRulesResponse,
        params: { id: rateId },
      }),
  })

  const rows = result.data ?? []

  return (
    <Section
      title={t("section.taxRules")}
      description={t("section.taxRulesWhy")}
    >
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          {t("general.loading")}
        </p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          No rules. This rate applies to everything in its region.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("field.kind")}</TableHead>
              <TableHead>{t("field.whichOne")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell>{row.reference}</TableCell>
                <TableCell>
                  <Mono>{row.reference_id}</Mono>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  )
}
