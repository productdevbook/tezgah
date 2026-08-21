import { useQuery } from "@tanstack/react-query"
import { z } from "zod"
import { useT } from "@/panel/i18n"

import { get } from "@/api/client"
import { taxRegistration, type TaxRegistration } from "@/api/schemas"
import { TableFrame } from "@/components/data-table"
import { QueryState } from "@/components/query-state"
import { Empty } from "@/components/detail-fields"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * `GET /admin/tax-registrations` answers a plain array, not `Page<T>` — no
 * cursor, so this is its own small `useQuery` rather than `usePagedList`, the
 * same reasoning as `features/store/currencies.tsx`. No `GET .../{id}` — only
 * `DELETE` reaches one by id — so a row here goes nowhere.
 */
export function TaxRegistrations() {
  const t = useT()
  const query = useQuery({
    queryKey: ["tax-registrations"],
    queryFn: ({ signal }) =>
      get("/admin/tax-registrations", {
        signal,
        schema: z.array(taxRegistration),
      }),
  })

  return (
    <QueryState
      query={query}
      empty={{
        title: t("empty.registrations"),
        description: t("empty.registrationsWhy"),
      }}
    >
      {(registrations: TaxRegistration[]) => (
        <TableFrame
          header={{
            title: t("frame.registrations"),
            description: t("frame.registrationsWhy"),
          }}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{t("field.country")}</TableHead>
                <TableHead>{t("field.scheme")}</TableHead>
                <TableHead>{t("field.taxId")}</TableHead>
                <TableHead className="text-right">{t("field.home")}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {registrations.map((registration) => (
                <TableRow key={registration.id}>
                  <TableCell className="font-mono text-xs uppercase">
                    {registration.country_code}
                  </TableCell>
                  <TableCell>{registration.scheme}</TableCell>
                  <TableCell className="font-mono text-xs">
                    {registration.tax_id ?? <Empty />}
                  </TableCell>
                  <TableCell className="text-right">
                    {registration.is_home ? <Badge>home</Badge> : null}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
    </QueryState>
  )
}
