import { useState, type FormEvent } from "react"

import { priceSet } from "@/api/schemas"
import { DetailField, FieldGrid } from "@/components/detail-fields"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

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
  const t = useT()
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
              placeholder={t("placeholder.priceSetId")}
              className="font-mono text-xs"
              aria-label={t("field.priceSetId")}
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
  const t = useT()
  const result = useDetail(
    ["price-sets"],
    "/admin/price-sets/{id}",
    priceSet,
    id
  )

  return (
    <QueryState
      query={result}
      empty={{
        title: t("empty.priceSet"),
        description: t("general.nothingToShow"),
      }}
    >
      {(item) => (
        <FieldGrid>
          <DetailField label={t("field.id")}>
            <span className="font-mono text-xs">{item.id}</span>
          </DetailField>
          <DetailField label={t("field.created")}>
            {dateTime(item.created_at)}
          </DetailField>
        </FieldGrid>
      )}
    </QueryState>
  )
}
