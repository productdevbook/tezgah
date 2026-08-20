import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { patch } from "@/api/client"
import { customer, updateCustomer, type Customer, type UpdateCustomer } from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { useDetail } from "@/lib/detail"

function displayName(row: Customer): string {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? row.email ?? "Unnamed customer")
}

/** `/customers/$id/edit` — the same address shape as `/products/$id/edit`. */
export function EditCustomer({ id }: { id: string }) {
  const result = useDetail(["customers"], "/admin/customers/{id}", customer, id)

  return (
    <RouteDrawer>
      <RouteDrawer.Header title="Edit customer" />
      <RouteDrawer.Body>
      <QueryState
        query={result}
        empty={{ title: "No customer", description: "Nothing to show." }}
      >
        {(item) => <CustomerForm item={item} />}
      </QueryState>
    </RouteDrawer.Body>
    </RouteDrawer>
  )
}

function CustomerForm({ item }: { item: Customer }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({
    first_name: item.first_name ?? "",
    last_name: item.last_name ?? "",
    email: item.email ?? "",
    phone: item.phone ?? "",
    company_name: item.company_name ?? "",
  })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: UpdateCustomer) =>
      patch("/admin/customers/{id}", { schema: customer, params: { id: item.id }, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["customers"] })
      void navigate({ to: "/customers/$id", params: { id: item.id } })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const empty = (v: string) => (v.trim() === "" ? null : v)
    const parsed = updateCustomer.safeParse({
      first_name: empty(form.first_name),
      last_name: empty(form.last_name),
      email: empty(form.email),
      phone: empty(form.phone),
      company_name: empty(form.company_name),
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
        <Field data-invalid={!!fieldErrors.email}>
          <FieldLabel htmlFor="customer-email">Email</FieldLabel>
          <Input
            id="customer-email"
            type="email"
            value={form.email}
            onChange={(e) => setForm({ ...form, email: e.target.value })}
            aria-invalid={!!fieldErrors.email}
          />
          <FieldError>{fieldErrors.email}</FieldError>
        </Field>
        <Field>
          <FieldLabel htmlFor="customer-first-name">First name</FieldLabel>
          <Input
            id="customer-first-name"
            value={form.first_name}
            onChange={(e) => setForm({ ...form, first_name: e.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="customer-last-name">Last name</FieldLabel>
          <Input
            id="customer-last-name"
            value={form.last_name}
            onChange={(e) => setForm({ ...form, last_name: e.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="customer-phone">Phone</FieldLabel>
          <Input
            id="customer-phone"
            value={form.phone}
            onChange={(e) => setForm({ ...form, phone: e.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="customer-company">Company</FieldLabel>
          <Input
            id="customer-company"
            value={form.company_name}
            onChange={(e) => setForm({ ...form, company_name: e.target.value })}
          />
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/customers/$id" params={{ id: item.id }} />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </>
  )
}
