import { useState, type FormEvent } from "react"

import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { useT } from "@/panel/i18n"

/**
 * `order_basket` (5 operations) has no `GET /admin/order-baskets` — the
 * crate does not carry one. `src/api/order_basket.rs` says why: "a shopper
 * never asks for a basket by id — the checkout that opened it hands the
 * order numbers back", so whoever needs one already has the id, from the
 * order it split into or the checkout that opened it. So the panel does not
 * fake a list here; an id, pasted in from wherever it was read, is the only
 * way in.
 */
export function BasketSearch({ onSearch }: { onSearch: (id: string) => void }) {
  const t = useT()
  const [id, setId] = useState("")

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = id.trim()
    if (trimmed !== "") onSearch(trimmed)
  }

  return (
    <div className="max-w-xl space-y-4">
      <PageHeading
        title={t("nav.baskets")}
        subtitle={t("screen.baskets.subtitle")}
      />
      <Card>
        <CardContent>
          <form className="space-y-4" onSubmit={submit}>
            <Field>
              <FieldLabel htmlFor="basket-id">{t("field.basketId")}</FieldLabel>
              <Input
                id="basket-id"
                value={id}
                onChange={(e) => setId(e.target.value)}
                placeholder={t("placeholder.basketId")}
                className="font-mono text-xs"
                autoFocus
              />
            </Field>
            <Button type="submit" disabled={id.trim() === ""}>
              Open
            </Button>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
