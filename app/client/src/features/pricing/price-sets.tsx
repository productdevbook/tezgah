import { useState, type FormEvent } from "react"

import { priceSet } from "@/api/schemas"
import { DetailField, FieldGrid } from "@/components/detail-fields"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { dateTime, useDetail } from "@/lib/detail"

/**
 * `price_set` (`src/pricing.rs`) has no `GET /admin/price-sets` — only
 * `POST` and `GET .../{id}` — so this is a lookup by id, the same shape as
 * `features/baskets/search.tsx`, not a list.
 */
export function PriceSets({
  id,
  onIdChange,
}: {
  id: string | undefined
  onIdChange: (id: string | undefined) => void
}) {
  const [input, setInput] = useState(id ?? "")

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    onIdChange(trimmed === "" ? undefined : trimmed)
  }

  return (
    <div className="max-w-xl space-y-4">
      <Card>
        <CardContent className="space-y-4">
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
          {id ? <PriceSetFields id={id} /> : null}
        </CardContent>
      </Card>
    </div>
  )
}

function PriceSetFields({ id }: { id: string }) {
  const result = useDetail(
    ["price-sets"],
    "/admin/price-sets/{id}",
    priceSet,
    id
  )

  return (
    <QueryState
      query={result}
      empty={{ title: "No price set", description: "Nothing to show." }}
    >
      {(item) => (
        <FieldGrid>
          <DetailField label="ID">
            <span className="font-mono text-xs">{item.id}</span>
          </DetailField>
          <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
        </FieldGrid>
      )}
    </QueryState>
  )
}
