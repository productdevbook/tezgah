import { useMutation, useQuery } from "@tanstack/react-query"
import { useState } from "react"

import { get } from "@/api/client"
import { inventoryLevel, page, stockLocation } from "@/api/schemas"
import { Empty } from "@/components/detail-fields"
import { Section } from "@/components/section"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { saveStockLevels, type StockLevelRow } from "@/features/inventory/stock"

/**
 * What is where, and the count changed in place.
 *
 * `POST /admin/inventory-items/batch` takes every level together, which is
 * what makes a grid possible here: a stock count is one act across a shelf,
 * and saving a row at a time would leave a half-counted item behind if one
 * of them were refused.
 *
 * `reserved` is not editable and never will be. A reservation belongs to an
 * order or a cart holding the stock; typing over the number would not release
 * anything, it would only make the row disagree with the thing holding it.
 */
export function Levels({ itemId }: { itemId: string }) {
  const levels = useQuery({
    queryKey: ["inventory-levels", itemId],
    queryFn: ({ signal }) =>
      get("/admin/inventory-items/{id}/location-levels", {
        signal,
        schema: page(inventoryLevel),
        params: { id: itemId },
        query: { limit: 100 },
      }),
  })

  // Every location, so a level says where it is rather than which uuid it is.
  // One request for the lot: a shop has a handful, and the alternative is one
  // lookup per row.
  const locations = useQuery({
    queryKey: ["stock-locations", "all"],
    queryFn: ({ signal }) =>
      get("/admin/stock-locations", {
        signal,
        schema: page(stockLocation),
        query: { limit: 100 },
      }),
  })

  const named = new Map(
    (locations.data?.items ?? []).map((one) => [one.id, one.name])
  )

  const rows = levels.data?.items ?? []

  return (
    <Section
      title="What is where"
      description="Counted per location. Type a count and save them together — one call, so a shelf is counted at once or not at all."
    >
      {levels.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          No location holds any of this yet. Stock arrives at a location, so
          until one does there is nothing to count.
        </p>
      ) : (
        <Grid rows={rows} named={named} onSaved={() => void levels.refetch()} />
      )}
    </Section>
  )
}

type Level = (typeof inventoryLevel)["_output"]

function Grid({
  rows,
  named,
  onSaved,
}: {
  rows: Level[]
  named: Map<string, string>
  onSaved: () => void
}) {
  // By location id, and absent means untouched — which is what keeps a save
  // to the counts somebody actually changed.
  const [counted, setCounted] = useState<Record<string, string>>({})
  const [incoming, setIncoming] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (levels: StockLevelRow[]) => saveStockLevels(levels),
    onSuccess: () => {
      setCounted({})
      setIncoming({})
      onSaved()
    },
  })

  const whole = (value: string) => /^\d+$/.test(value)

  const changed = rows.filter((row) => {
    const a = counted[row.location_id]
    const b = incoming[row.location_id]
    return (
      (a !== undefined && a !== String(row.stocked_quantity)) ||
      (b !== undefined && b !== String(row.incoming_quantity))
    )
  })

  const malformed = changed.filter((row) => {
    const a = counted[row.location_id] ?? String(row.stocked_quantity)
    const b = incoming[row.location_id] ?? String(row.incoming_quantity)
    return !whole(a) || !whole(b)
  })

  return (
    <>
      {changed.length > 0 ? (
        <div className="flex items-center gap-2 px-6 pb-3">
          <span className="text-sm text-muted-foreground">
            {changed.length} changed
          </span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => {
              setCounted({})
              setIncoming({})
            }}
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
                  inventory_item_id: row.inventory_item_id,
                  location_id: row.location_id,
                  stocked_quantity: Number(
                    counted[row.location_id] ?? row.stocked_quantity
                  ),
                  incoming_quantity: Number(
                    incoming[row.location_id] ?? row.incoming_quantity
                  ),
                }))
              )
            }
          >
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
        </div>
      ) : null}

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Location</TableHead>
            <TableHead className="w-28">Counted</TableHead>
            <TableHead className="w-28">Incoming</TableHead>
            <TableHead className="text-right">Reserved</TableHead>
            <TableHead className="text-right">Available</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => {
            const stocked = counted[row.location_id]
            const arriving = incoming[row.location_id]
            return (
              <TableRow key={row.id}>
                <TableCell>{named.get(row.location_id) ?? <Empty />}</TableCell>
                <TableCell>
                  <Input
                    className="h-8 font-mono text-xs"
                    inputMode="numeric"
                    aria-label={`Counted at ${named.get(row.location_id) ?? "this location"}`}
                    aria-invalid={stocked !== undefined && !whole(stocked)}
                    value={stocked ?? String(row.stocked_quantity)}
                    onChange={(event) =>
                      setCounted((was) => ({
                        ...was,
                        [row.location_id]: event.target.value,
                      }))
                    }
                  />
                </TableCell>
                <TableCell>
                  <Input
                    className="h-8 font-mono text-xs"
                    inputMode="numeric"
                    aria-label={`Incoming at ${named.get(row.location_id) ?? "this location"}`}
                    aria-invalid={arriving !== undefined && !whole(arriving)}
                    value={arriving ?? String(row.incoming_quantity)}
                    onChange={(event) =>
                      setIncoming((was) => ({
                        ...was,
                        [row.location_id]: event.target.value,
                      }))
                    }
                  />
                </TableCell>
                <TableCell className="text-right font-mono text-xs text-muted-foreground">
                  {row.reserved_quantity}
                </TableCell>
                {/* Negative means a backorder was allowed, and the number
                    says so rather than clamping at none. */}
                <TableCell
                  className={
                    row.available_quantity < 0
                      ? "text-right font-mono text-xs text-destructive"
                      : "text-right font-mono text-xs"
                  }
                >
                  {row.available_quantity}
                </TableCell>
              </TableRow>
            )
          })}
        </TableBody>
      </Table>

      {malformed.length > 0 ? (
        <p className="px-6 py-3 text-sm text-destructive">
          A count is a whole number of things. Nothing is sent until every one
          of them is.
        </p>
      ) : null}

      {mutation.isError ? (
        <p className="px-6 py-3 text-sm text-destructive">
          {mutation.error instanceof Error
            ? mutation.error.message
            : "Refused."}
        </p>
      ) : null}

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
    </>
  )
}
