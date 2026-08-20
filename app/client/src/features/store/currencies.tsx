import { useQuery } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"
import { z } from "zod"

import { get } from "@/api/client"
import { currency, type Currency } from "@/api/schemas"
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
  const query = useQuery({
    queryKey: ["currencies"],
    queryFn: ({ signal }) =>
      get("/admin/currencies", { signal, schema: z.array(currency) }),
  })

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <Button
          size="sm"
          variant="outline"
          nativeButton={false}
          render={<Link to="/store/currencies/new" />}
        >
          <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          New currency
        </Button>
      </div>
      <QueryState
        query={query}
        empty={{
          title: "No currencies",
          description: "Nothing prices or opens a cart until a shop enables one.",
        }}
      >
        {(currencies: Currency[]) => (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Code</TableHead>
                  <TableHead>Name</TableHead>
                  <TableHead>Symbol</TableHead>
                  <TableHead className="text-right">Exponent</TableHead>
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
          </div>
        )}
      </QueryState>
    </div>
  )
}
