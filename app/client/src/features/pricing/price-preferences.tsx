import { useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"
import { useT } from "@/panel/i18n"

import { get } from "@/api/client"
import { pricePreference } from "@/api/schemas"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

/**
 * `GET /admin/price-preferences` wants `attribute` (and takes an optional
 * `value`) — `src/api/admin_catalogue.rs`'s `FindPricePreference` — so this
 * is a lookup, the same shape as `features/payouts/screen.tsx`'s
 * `BalanceLookup`, not a list: nothing here browses every preference.
 */
export function PricePreferences({
  attribute,
  onAttributeChange,
  value,
  onValueChange,
}: {
  attribute: string | undefined
  onAttributeChange: (attribute: string | undefined) => void
  value: string | undefined
  onValueChange: (value: string | undefined) => void
}) {
  const t = useT()
  const [attributeInput, setAttributeInput] = useState(attribute ?? "")
  const [valueInput, setValueInput] = useState(value ?? "")

  const query = useQuery({
    queryKey: ["price-preferences", attribute, value],
    queryFn: ({ signal }) =>
      get("/admin/price-preferences", {
        signal,
        schema: pricePreference,
        query: { attribute, value },
      }),
    enabled: attribute !== undefined,
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmedAttribute = attributeInput.trim()
    const trimmedValue = valueInput.trim()
    onAttributeChange(trimmedAttribute === "" ? undefined : trimmedAttribute)
    onValueChange(trimmedValue === "" ? undefined : trimmedValue)
  }

  return (
    <div className="max-w-xl space-y-4">
      <Card>
        <CardContent className="space-y-4">
          <form className="space-y-4" onSubmit={submit}>
            <Field>
              <FieldLabel htmlFor="preference-attribute">
                {t("field.attribute")}
              </FieldLabel>
              <Input
                id="preference-attribute"
                value={attributeInput}
                onChange={(e) => setAttributeInput(e.target.value)}
                placeholder="shipping_method, product_type, ..."
                className="font-mono text-xs"
                autoFocus
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="preference-value">
                Value (optional)
              </FieldLabel>
              <Input
                id="preference-value"
                value={valueInput}
                onChange={(e) => setValueInput(e.target.value)}
                placeholder={t("placeholder.leftOutPreference")}
                className="font-mono text-xs"
              />
            </Field>
            <Button type="submit" disabled={attributeInput.trim() === ""}>
              Look up
            </Button>
          </form>
          {attribute !== undefined ? (
            <QueryState
              query={query}
              empty={{
                title: t("empty.pricePreference"),
                description: t("empty.pricePreferenceWhy"),
              }}
            >
              {(preference) =>
                preference === null ? (
                  <p className="text-sm text-muted-foreground">
                    No preference set for this attribute.
                  </p>
                ) : (
                  <p className="text-sm">
                    <span className="font-mono text-xs">
                      {preference.attribute}
                    </span>
                    {preference.value ? (
                      <>
                        {" = "}
                        <span className="font-mono text-xs">
                          {preference.value}
                        </span>
                      </>
                    ) : null}{" "}
                    is quoted{" "}
                    {preference.is_tax_inclusive
                      ? "tax inclusive"
                      : "tax exclusive"}
                    .
                  </p>
                )
              }
            </QueryState>
          ) : null}
        </CardContent>
      </Card>
    </div>
  )
}
