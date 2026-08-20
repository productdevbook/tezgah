import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { patch } from "@/api/client"
import { region, updateRegion, type Region, type UpdateRegion } from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useDetail } from "@/lib/detail"

/**
 * `/store/regions/$id/edit` — the trailing-underscore escape `region-detail.tsx`
 * already takes, for the same reason: a full page, not a `/store` tab.
 *
 * `tezgah::api` has no route to delete a region, only one to take a country
 * out of it (`server/README.md`) — so this screen, unlike the other four,
 * offers no delete action.
 */
export function EditRegion({ id }: { id: string }) {
  const result = useDetail(["regions"], "/admin/regions/{id}", region, id)

  return (
    <div className="max-w-xl space-y-4">
      <QueryState query={result} empty={{ title: "No region", description: "Nothing to show." }}>
        {(item) => <RegionForm item={item} />}
      </QueryState>
    </div>
  )
}

function RegionForm({ item }: { item: Region }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({
    name: item.name,
    currency_code: item.currency_code,
    is_tax_inclusive: item.is_tax_inclusive,
    has_automatic_taxes: item.has_automatic_taxes,
  })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: UpdateRegion) =>
      patch("/admin/regions/{id}", { schema: region, params: { id: item.id }, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["regions"] })
      void navigate({ to: "/store/regions/$id", params: { id: item.id } })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = updateRegion.safeParse(form)
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
    <>
      <PageHeading title={`Edit ${item.name}`} />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="region-name">Name</FieldLabel>
          <Input
            id="region-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
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
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/regions/$id" params={{ id: item.id }} />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </>
  )
}
