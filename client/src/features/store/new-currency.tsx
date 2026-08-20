import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { post } from "@/api/client"
import {
  createCurrency,
  currency,
  type CreateCurrency,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

const EMPTY_FORM = {
  code: "",
  numeric_code: "",
  exponent: "2",
  symbol: "",
  symbol_native: "",
  name: "",
}

export function NewCurrency() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState(EMPTY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateCurrency) =>
      post("/admin/currencies", { schema: currency, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["currencies"] })
      void navigate({ to: "/store/currencies" })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createCurrency.safeParse({
      code: form.code,
      numeric_code:
        form.numeric_code.trim() === "" ? undefined : form.numeric_code,
      exponent: Number(form.exponent),
      symbol: form.symbol,
      symbol_native: form.symbol_native,
      name: form.name,
    })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <div className="max-w-xl space-y-4">
      <PageHeading
        title="Enable a currency"
        subtitle="tezgah keeps no built-in list. Enabling one twice corrects it rather than conflicting."
      />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <div className="grid grid-cols-2 gap-4">
          <Field data-invalid={!!fieldErrors.code}>
            <FieldLabel htmlFor="currency-code">Code</FieldLabel>
            <Input
              id="currency-code"
              value={form.code}
              onChange={(e) => setForm({ ...form, code: e.target.value })}
              placeholder="USD"
              maxLength={3}
              aria-invalid={!!fieldErrors.code}
            />
            <FieldError>{fieldErrors.code}</FieldError>
          </Field>
          <Field data-invalid={!!fieldErrors.exponent}>
            <FieldLabel htmlFor="currency-exponent">Exponent</FieldLabel>
            <Input
              id="currency-exponent"
              type="number"
              min={0}
              max={4}
              value={form.exponent}
              onChange={(e) => setForm({ ...form, exponent: e.target.value })}
              aria-invalid={!!fieldErrors.exponent}
            />
            <FieldError>{fieldErrors.exponent}</FieldError>
          </Field>
        </div>
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="currency-name">Name</FieldLabel>
          <Input
            id="currency-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="US Dollar"
            aria-invalid={!!fieldErrors.name}
          />
          <FieldError>{fieldErrors.name}</FieldError>
        </Field>
        <div className="grid grid-cols-2 gap-4">
          <Field data-invalid={!!fieldErrors.symbol}>
            <FieldLabel htmlFor="currency-symbol">Symbol</FieldLabel>
            <Input
              id="currency-symbol"
              value={form.symbol}
              onChange={(e) => setForm({ ...form, symbol: e.target.value })}
              placeholder="$"
              aria-invalid={!!fieldErrors.symbol}
            />
            <FieldError>{fieldErrors.symbol}</FieldError>
          </Field>
          <Field data-invalid={!!fieldErrors.symbol_native}>
            <FieldLabel htmlFor="currency-symbol-native">Native symbol</FieldLabel>
            <Input
              id="currency-symbol-native"
              value={form.symbol_native}
              onChange={(e) => setForm({ ...form, symbol_native: e.target.value })}
              placeholder="$"
              aria-invalid={!!fieldErrors.symbol_native}
            />
            <FieldError>{fieldErrors.symbol_native}</FieldError>
          </Field>
        </div>
        <Field>
          <FieldLabel htmlFor="currency-numeric">Numeric code (optional)</FieldLabel>
          <Input
            id="currency-numeric"
            value={form.numeric_code}
            onChange={(e) => setForm({ ...form, numeric_code: e.target.value })}
            placeholder="840"
            maxLength={3}
          />
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Enabling…" : "Enable currency"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/currencies" />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  )
}
