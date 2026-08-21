import { useQuery } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"
import { z } from "zod"
import { useT } from "@/panel/i18n"

import { get } from "@/api/client"
import { currency, type Currency } from "@/api/schemas"
import { TableFrame } from "@/components/data-table"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * `GET /admin/currencies` answers a plain array, not `Page<T>`
 * (`server/src/http/admin.rs`'s `list_currencies`) — no cursor, so this is
 * its own small `useQuery` rather than `usePagedList`.
 */
export function StoreCurrencies() {
  const t = useT()
  const query = useQuery({
    queryKey: ["currencies"],
    queryFn: ({ signal }) =>
      get("/admin/currencies", { signal, schema: z.array(currency) }),
  })

  return (
    <div className="space-y-3">
      <QueryState
        query={query}
        empty={{
          title: t("empty.currencies"),
          description: t("empty.currenciesWhy"),
        }}
      >
        {(currencies: Currency[]) => (
          <TableFrame
            header={{
              title: t("frame.currencies"),
              description: t("frame.currenciesWhy"),
              actions: (
                <Button
                  size="sm"
                  variant="outline"
                  nativeButton={false}
                  render={<Link to="/store/currencies/new" />}
                >
                  <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
                  New currency
                </Button>
              ),
            }}
          >
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("field.code")}</TableHead>
                  <TableHead>{t("field.name")}</TableHead>
                  <TableHead>{t("field.symbol")}</TableHead>
                  <TableHead className="text-right">
                    {t("field.exponent")}
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {currencies.map((c) => (
                  <TableRow key={c.code}>
                    <TableCell className="font-mono text-xs uppercase">
                      {c.code}
                    </TableCell>
                    <TableCell className="font-medium">{c.name}</TableCell>
                    <TableCell>{c.symbol}</TableCell>
                    <TableCell className="text-right text-xs text-muted-foreground">
                      {c.exponent}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableFrame>
        )}
      </QueryState>
    </div>
  )
}
