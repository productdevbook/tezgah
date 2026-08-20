import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { patch } from "@/api/client"
import {
  promotion,
  updatePromotion,
  type Promotion,
  type UpdatePromotion,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useDetail } from "@/lib/detail"

/** `/promotions/$id/edit` — the same address shape as `/products/$id/edit`. */
export function EditPromotion({ id }: { id: string }) {
  const result = useDetail(["promotions"], "/admin/promotions/{id}", promotion, id)

  return (
    <RouteDrawer>
      <RouteDrawer.Header title="Edit promotion" />
      <RouteDrawer.Body>
      <QueryState
        query={result}
        empty={{ title: "No promotion", description: "Nothing to show." }}
      >
        {(item) => <PromotionForm item={item} />}
      </QueryState>
    </RouteDrawer.Body>
    </RouteDrawer>
  )
}

function PromotionForm({ item }: { item: Promotion }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({
    code: item.code,
    is_automatic: item.is_automatic,
    usage_limit: item.usage_limit === null ? "" : String(item.usage_limit),
    customer_usage_limit:
      item.customer_usage_limit === null ? "" : String(item.customer_usage_limit),
  })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: UpdatePromotion) =>
      patch("/admin/promotions/{id}", { schema: promotion, params: { id: item.id }, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["promotions"] })
      void navigate({ to: "/promotions/$id", params: { id: item.id } })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const asLimit = (v: string) => (v.trim() === "" ? null : Number(v))
    const parsed = updatePromotion.safeParse({
      code: form.code,
      is_automatic: form.is_automatic,
      usage_limit: asLimit(form.usage_limit),
      customer_usage_limit: asLimit(form.customer_usage_limit),
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
    <>
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.code}>
          <FieldLabel htmlFor="promotion-code">Code</FieldLabel>
          <Input
            id="promotion-code"
            value={form.code}
            onChange={(e) => setForm({ ...form, code: e.target.value })}
            aria-invalid={!!fieldErrors.code}
          />
          <FieldError>{fieldErrors.code}</FieldError>
        </Field>
        <Field orientation="horizontal">
          <Switch
            id="promotion-automatic"
            checked={form.is_automatic}
            onCheckedChange={(checked) => setForm({ ...form, is_automatic: checked })}
          />
          <FieldLabel htmlFor="promotion-automatic">Applies automatically</FieldLabel>
        </Field>
        <Field data-invalid={!!fieldErrors.usage_limit}>
          <FieldLabel htmlFor="promotion-usage-limit">Usage limit</FieldLabel>
          <Input
            id="promotion-usage-limit"
            type="number"
            min={0}
            value={form.usage_limit}
            onChange={(e) => setForm({ ...form, usage_limit: e.target.value })}
            placeholder="Unlimited"
            aria-invalid={!!fieldErrors.usage_limit}
          />
          <FieldError>{fieldErrors.usage_limit}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.customer_usage_limit}>
          <FieldLabel htmlFor="promotion-customer-usage-limit">
            Per-customer limit
          </FieldLabel>
          <Input
            id="promotion-customer-usage-limit"
            type="number"
            min={0}
            value={form.customer_usage_limit}
            onChange={(e) => setForm({ ...form, customer_usage_limit: e.target.value })}
            placeholder="Unlimited"
            aria-invalid={!!fieldErrors.customer_usage_limit}
          />
          <FieldError>{fieldErrors.customer_usage_limit}</FieldError>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/promotions/$id" params={{ id: item.id }} />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </>
  )
}
