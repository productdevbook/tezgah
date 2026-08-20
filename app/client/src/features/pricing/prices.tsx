import { useMutation } from "@tanstack/react-query"
import { useState, type FormEvent } from "react"

import { price, type Price } from "@/api/schemas"
import { DataTable, TableFrame, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { usePagedList } from "@/lib/paged"
import { savePrices, type PriceChange } from "@/features/pricing/grid"
import { PriceRules } from "@/features/pricing/rules"

const columns: Columns<Price> = [
  {
    header: "Amount",
    accessorKey: "amount",
    cell: ({ row }) =>
      `${row.original.amount} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Quantity",
    accessorKey: "min_quantity",
    cell: ({ row }) =>
      row.original.min_quantity === null &&
      row.original.max_quantity === null ? (
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
    cell: ({ row }) => (
      <PriceRules priceId={row.original.id} count={row.original.rules_count} />
    ),
    meta: { className: "text-right" },
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
        <PricesInSet
          priceSetId={priceSetId}
          after={after}
          onAfterChange={onAfterChange}
        />
      ) : (
        <p className="text-sm text-muted-foreground">
          Prices are listed by the set that owns them — paste a price set's id
          above.
        </p>
      )}
    </div>
  )
}

/**
 * The edit grid: every price in a set, its amount typed in place, and one
 * call that writes all of them.
 *
 * This is the shape a shop actually changes prices in, and the reason it can
 * exist here is that `POST /admin/prices/batch` takes them together — a grid
 * that saved a row at a time would be a hundred requests and a half-applied
 * page if one of them failed.
 *
 * Only the amount is editable. The currency, the quantity band and the rules
 * are what make a price the price it is; changing one of those is making a
 * different price, which is a different call.
 */
function PricesInSet({
  priceSetId,
  after,
  onAfterChange,
}: {
  priceSetId: string
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(
    ["price-set-prices", priceSetId],
    "/admin/price-sets/{id}/prices",
    price,
    { after, onAfterChange, params: { id: priceSetId } }
  )

  const rows = paged.result.data?.items
  if (!rows || rows.length === 0) {
    return (
      <DataTable
        paged={paged}
        columns={columns}
        empty={{
          title: "No prices",
          description: "This price set has no prices yet.",
        }}
      />
    )
  }

  return <Grid rows={rows} onSaved={() => void paged.result.refetch()} />
}

function Grid({ rows, onSaved }: { rows: Price[]; onSaved: () => void }) {
  // What has been typed, by price id. Absent means untouched — which is what
  // keeps a save to the rows somebody actually changed rather than writing
  // every row back with the value it already had.
  const [edited, setEdited] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (changes: PriceChange[]) => savePrices(changes),
    onSuccess: () => {
      setEdited({})
      onSaved()
    },
  })

  const changed = rows.filter(
    (row) => edited[row.id] !== undefined && edited[row.id] !== row.amount
  )

  const malformed = changed.filter(
    (row) => !/^\d+(\.\d+)?$/.test(edited[row.id] ?? "")
  )

  return (
    <TableFrame
      header={{
        title: "Prices",
        description:
          "Type an amount and save them together. Only the amount is editable — a currency or a quantity band is what makes a price that price.",
        actions:
          changed.length > 0 ? (
            <>
              <span className="text-sm text-muted-foreground">
                {changed.length} changed
              </span>
              <Button
                size="sm"
                variant="outline"
                onClick={() => setEdited({})}
                disabled={mutation.isPending}
              >
                Discard
              </Button>
              <Button
                size="sm"
                disabled={mutation.isPending || malformed.length > 0}
                onClick={() =>
                  mutation.mutate(
                    changed.map((row) => ({
                      id: row.id,
                      amount: edited[row.id] ?? row.amount,
                      currency_code: row.currency_code,
                    }))
                  )
                }
              >
                {mutation.isPending ? "Saving…" : "Save"}
              </Button>
            </>
          ) : undefined,
      }}
    >
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead className="w-48">Amount</TableHead>
            <TableHead>Currency</TableHead>
            <TableHead>Quantity</TableHead>
            <TableHead>Title</TableHead>
            <TableHead className="text-right">Rules</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const typed = edited[row.id]
            const wrong = typed !== undefined && !/^\d+(\.\d+)?$/.test(typed)
            return (
              <TableRow key={row.id}>
                <TableCell>
                  <Input
                    className="h-8 font-mono text-xs"
                    inputMode="decimal"
                    aria-label={`Amount in ${row.currency_code.toUpperCase()}`}
                    aria-invalid={wrong}
                    value={typed ?? row.amount}
                    onChange={(event) =>
                      setEdited((was) => ({
                        ...was,
                        [row.id]: event.target.value,
                      }))
                    }
                  />
                </TableCell>
                <TableCell className="font-mono text-xs uppercase">
                  {row.currency_code}
                </TableCell>
                <TableCell>
                  {row.min_quantity === null && row.max_quantity === null ? (
                    <Empty />
                  ) : (
                    `${row.min_quantity ?? "0"}–${row.max_quantity ?? "∞"}`
                  )}
                </TableCell>
                <TableCell>{row.title ?? <Empty />}</TableCell>
                <TableCell className="text-right">
                  <PriceRules priceId={row.id} count={row.rules_count} />
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>

      {malformed.length > 0 ? (
        <p className="px-6 py-3 text-sm text-destructive">
          {malformed.length} amount{malformed.length === 1 ? " is" : "s are"}{" "}
          not a number. Nothing is sent until every one of them is.
        </p>
      ) : null}

      {mutation.isError ? (
        <p className="px-6 py-3 text-sm text-destructive">
          {mutation.error instanceof Error
            ? mutation.error.message
            : "Refused."}
        </p>
      ) : null}

      {/* A batch that half worked is the ordinary case: the crate applies
          what it can and names the rows it could not. */}
      {mutation.data && mutation.data.rejected.length > 0 ? (
        <div className="px-6 py-3">
          <p className="text-sm text-destructive">
            {mutation.data.applied} applied, {mutation.data.rejected.length}{" "}
            refused:
          </p>
          <ul className="mt-1 list-disc pl-5 text-sm text-muted-foreground">
            {mutation.data.rejected.map((one) => (
              <li key={`${one.row}-${one.reason}`}>
                row {one.row}: {one.reason}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </TableFrame>
  )
}
