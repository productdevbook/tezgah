import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { post } from "@/api/client"
import { createRegion, region, type CreateRegion } from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"

const EMPTY_FORM = {
  name: "",
  currency_code: "",
  is_tax_inclusive: false,
  has_automatic_taxes: true,
}

export function NewRegion() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState(EMPTY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateRegion) =>
      post("/admin/regions", { schema: region, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["regions"] })
      void navigate({ to: "/store/regions" })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createRegion.safeParse(form)
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
        title="New region"
        subtitle="A region decides currency and how tax is shown."
      />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="region-name">Name</FieldLabel>
          <Input
            id="region-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Europe"
            aria-invalid={!!fieldErrors.name}
          />
          <FieldError>{fieldErrors.name}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.currency_code}>
          <FieldLabel htmlFor="region-currency">Currency code</FieldLabel>
          <Input
            id="region-currency"
            value={form.currency_code}
            onChange={(e) => setForm({ ...form, currency_code: e.target.value })}
            placeholder="EUR"
            maxLength={3}
            aria-invalid={!!fieldErrors.currency_code}
          />
          <FieldError>{fieldErrors.currency_code}</FieldError>
        </Field>
        <Field orientation="horizontal">
          <Switch
            id="region-tax-inclusive"
            checked={form.is_tax_inclusive}
            onCheckedChange={(checked) => setForm({ ...form, is_tax_inclusive: checked })}
          />
          <FieldLabel htmlFor="region-tax-inclusive">Prices include tax</FieldLabel>
        </Field>
        <Field orientation="horizontal">
          <Switch
            id="region-automatic-taxes"
            checked={form.has_automatic_taxes}
            onCheckedChange={(checked) =>
              setForm({ ...form, has_automatic_taxes: checked })
            }
          />
          <FieldLabel htmlFor="region-automatic-taxes">Automatic taxes</FieldLabel>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Creating…" : "Create region"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/regions" />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  )
}
