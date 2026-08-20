import { useState, type FormEvent } from "react"

import { price, type Price } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { usePagedList } from "@/lib/paged"

const columns: Columns<Price> = [
  {
    header: "Amount",
    accessorKey: "amount",
    cell: ({ row }) => `${row.original.amount} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Quantity",
    accessorKey: "min_quantity",
    cell: ({ row }) =>
      row.original.min_quantity === null && row.original.max_quantity === null ? (
        <Empty />
      ) : (
        `${row.original.min_quantity ?? "0"}–${row.original.max_quantity ?? "∞"}`
      ),
  },
  {
    header: "Title",
    accessorKey: "title",
    cell: ({ row }) => row.original.title ?? <Empty />,
  },
  {
    header: "Rules",
    accessorKey: "rules_count",
    meta: { className: "text-right font-mono text-xs" },
  },
]

/**
 * `GET /admin/prices` does not exist — a price is only listed scoped to the
 * set that owns it (`GET /admin/price-sets/{id}/prices`), and there is no
 * `GET /admin/prices/{id}` either, so a row here goes nowhere.
 */
export function Prices({
  priceSetId,
  onPriceSetIdChange,
  after,
  onAfterChange,
}: {
  priceSetId: string | undefined
  onPriceSetIdChange: (id: string | undefined) => void
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const [input, setInput] = useState(priceSetId ?? "")

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    onPriceSetIdChange(trimmed === "" ? undefined : trimmed)
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardContent>
          <form className="flex gap-2" onSubmit={submit}>
            <Input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="price set id"
              className="font-mono text-xs"
              aria-label="Price set id"
              autoFocus
            />
            <Button type="submit" variant="outline">
              Look up
            </Button>
          </form>
        </CardContent>
      </Card>
      {priceSetId ? (
        <PricesInSet priceSetId={priceSetId} after={after} onAfterChange={onAfterChange} />
      ) : (
        <p className="text-muted-foreground text-sm">
          Prices are listed by the set that owns them — paste a price set's id above.
        </p>
      )}
    </div>
  )
}

function PricesInSet({
  priceSetId,
  after,
  onAfterChange,
}: {
  priceSetId: string
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["price-set-prices", priceSetId], "/admin/price-sets/{id}/prices", price, {
    after,
    onAfterChange,
    params: { id: priceSetId },
  })

  return (
    <DataTable
      paged={paged}
      columns={columns}
      empty={{ title: "No prices", description: "This price set has no prices yet." }}
    />
  )
}
