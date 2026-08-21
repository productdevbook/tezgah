import { useQuery } from "@tanstack/react-query"

import { z } from "zod"

import { get, type ApiPath } from "@/api/client"
import { page } from "@/api/schemas"
import { Empty, Mono } from "@/components/detail-fields"
import { Section } from "@/components/section"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime } from "@/lib/detail"
import { useT } from "@/panel/i18n"

/**
 * What moved on a balance, newest last.
 *
 * A gift card and a store credit answer the same view from two routes, so
 * this is one component given the path — the shapes are identical in the
 * document and pretending otherwise would be two of the same screen.
 *
 * The page schema is used whole rather than an item schema: the document
 * describes the page, and no route answers one movement on its own to name
 * an item after.
 */
/**
 * `CreditMovementView`, written out here rather than imported.
 *
 * The document describes it only inside a page, and a page's generated
 * schema is an intersection — `{items: unknown[]} & {items: Movement[]}` —
 * whose element type TypeScript resolves back to `unknown`. So the generated
 * page cannot be used whole, and no route answers one movement on its own to
 * name an item schema after: adjusting a balance returns the balance.
 *
 * A wrong field here is caught the first time a row arrives, by
 * `parseResponse`, and says which field it was.
 */
const movement = z.object({
  id: z.string(),
  amount: z.string(),
  currency_code: z.string(),
  kind: z.string(),
  order_id: z.string().nullable(),
  reason: z.string().nullable(),
  created_at: z.string(),
})

export function Movements({
  path,
  id,
  bare = false,
}: {
  path: ApiPath
  id: string
  /** Inside a section that already has a heading, rather than being one. */
  bare?: boolean
}) {
  const t = useT()
  const result = useQuery({
    queryKey: ["credit-movements", path, id],
    queryFn: ({ signal }) =>
      get(path, {
        signal,
        schema: page(movement),
        params: { id },
        query: { limit: 50 },
      }),
  })

  const rows = result.data?.items ?? []

  const body = (
    <>
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          Nothing has moved yet.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>When</TableHead>
              <TableHead>Kind</TableHead>
              <TableHead>Order</TableHead>
              <TableHead>Reason</TableHead>
              <TableHead className="text-right">Amount</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell className="text-xs text-muted-foreground">
                  {dateTime(row.created_at)}
                </TableCell>
                <TableCell>{row.kind}</TableCell>
                <TableCell>
                  {row.order_id ? <Mono>{row.order_id}</Mono> : <Empty />}
                </TableCell>
                <TableCell className="text-muted-foreground">
                  {row.reason ?? <Empty />}
                </TableCell>
                {/* A spend is negative and shows as one. Turning it into a
                    positive under a "spent" heading would make the column
                    stop adding up to the balance. */}
                <TableCell
                  className={
                    row.amount.startsWith("-")
                      ? "text-right font-mono text-xs text-destructive"
                      : "text-right font-mono text-xs"
                  }
                >
                  {row.amount} {row.currency_code.toUpperCase()}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </>
  )

  if (bare) return body

  return (
    <Section
      title={t("section.movements")}
      description={t("section.movementsWhy")}
    >
      {body}
    </Section>
  )
}
