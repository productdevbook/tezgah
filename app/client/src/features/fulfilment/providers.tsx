import { useQuery } from "@tanstack/react-query"
import { z } from "zod"

import { get } from "@/api/client"
import { fulfilmentProvider, type FulfilmentProvider } from "@/api/schemas"
import { TableFrame } from "@/components/data-table"
import { QueryState } from "@/components/query-state"
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
 * `GET /admin/fulfillment-providers` answers a plain array, not `Page<T>` —
 * no cursor, so this is its own small `useQuery` rather than `usePagedList`,
 * the same reasoning as `features/store/currencies.tsx`. No `GET .../{id}`
 * exists, so a row here goes nowhere.
 */
export function FulfilmentProviders() {
  const query = useQuery({
    queryKey: ["fulfilment-providers"],
    queryFn: ({ signal }) =>
      get("/admin/fulfillment-providers", {
        signal,
        schema: z.array(fulfilmentProvider),
      }),
  })

  return (
    <QueryState
      query={query}
      empty={{
        title: "No carriers",
        description: "Nothing ships until a shop turns a provider on.",
      }}
    >
      {(providers: FulfilmentProvider[]) => (
        <TableFrame
          header={{
            title: "Carriers",
            description: "Nothing ships until a shop turns a provider on.",
          }}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Code</TableHead>
                <TableHead className="text-right">Status</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {providers.map((provider) => (
                <TableRow key={provider.id}>
                  <TableCell className="font-mono text-xs">
                    {provider.code}
                  </TableCell>
                  <TableCell className="text-right">
                    <Badge
                      variant={provider.is_enabled ? "default" : "outline"}
                    >
                      {provider.is_enabled ? "enabled" : "disabled"}
                    </Badge>
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
