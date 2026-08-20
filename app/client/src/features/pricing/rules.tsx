import { useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { get } from "@/api/client"
import { GetAdminPricesByIdRulesResponse } from "@/api/generated/zod/pricing/pricing"
import { Mono } from "@/components/detail-fields"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"

/**
 * What decides whether a price is the one that applies.
 *
 * The grid has carried a count of these since it was written and no way to
 * see them — `GET /admin/prices/{id}/rules` is bound and was drawn by
 * nothing. A count with nothing behind it is a number a shop cannot act on.
 *
 * A dialog rather than a screen of its own: a price has no `GET
 * /admin/prices/{id}`, so there is no page for one to live on, and the rules
 * are read while looking at the row they belong to.
 */
export function PriceRules({
  priceId,
  count,
}: {
  priceId: string
  count: number
}) {
  const [open, setOpen] = useState(false)

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        className="h-7 font-mono text-xs"
        onClick={() => setOpen(true)}
        disabled={count === 0}
        aria-label={count === 0 ? "No rules" : `Show ${count} rules`}
      >
        {count}
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>What this price applies to</DialogTitle>
            <DialogDescription>
              Every rule has to hold for the price to be the one used. A price
              with none applies whenever its currency and quantity band do.
            </DialogDescription>
          </DialogHeader>
          {open ? <Rules priceId={priceId} /> : null}
        </DialogContent>
      </Dialog>
    </>
  )
}

function Rules({ priceId }: { priceId: string }) {
  const result = useQuery({
    queryKey: ["price-rules", priceId],
    queryFn: ({ signal }) =>
      get("/admin/prices/{id}/rules", {
        signal,
        schema: GetAdminPricesByIdRulesResponse,
        params: { id: priceId },
      }),
  })

  const rows = result.data ?? []

  if (result.isPending) {
    return <p className="text-sm text-muted-foreground">Loading…</p>
  }

  if (rows.length === 0) {
    return <p className="text-sm text-muted-foreground">No rules.</p>
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Attribute</TableHead>
          <TableHead>Is</TableHead>
          <TableHead>Value</TableHead>
          <TableHead className="text-right">Priority</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {rows.map((row) => (
          <TableRow key={row.id}>
            <TableCell className="font-medium">{row.attribute}</TableCell>
            <TableCell className="text-muted-foreground">
              {row.operator}
            </TableCell>
            <TableCell>
              <Mono>{row.value}</Mono>
            </TableCell>
            <TableCell className="text-right font-mono text-xs text-muted-foreground">
              {row.priority}
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
