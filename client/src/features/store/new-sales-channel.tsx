import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { post } from "@/api/client"
import {
  createSalesChannel,
  salesChannel,
  type CreateSalesChannel,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"

const EMPTY_FORM = { name: "", description: "", is_disabled: false }

export function NewSalesChannel() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState(EMPTY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateSalesChannel) =>
      post("/admin/sales-channels", { schema: salesChannel, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["sales-channels"] })
      void navigate({ to: "/store/sales-channels" })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createSalesChannel.safeParse({
      name: form.name,
      description: form.description.trim() === "" ? undefined : form.description,
      is_disabled: form.is_disabled,
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
        title="New sales channel"
        subtitle="A channel decides which products a storefront can see."
      />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="channel-name">Name</FieldLabel>
          <Input
            id="channel-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            placeholder="Web storefront"
            aria-invalid={!!fieldErrors.name}
          />
          <FieldError>{fieldErrors.name}</FieldError>
        </Field>
        <Field>
          <FieldLabel htmlFor="channel-description">Description</FieldLabel>
          <Input
            id="channel-description"
            value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
          />
        </Field>
        <Field orientation="horizontal">
          <Switch
            id="channel-disabled"
            checked={form.is_disabled}
            onCheckedChange={(checked) => setForm({ ...form, is_disabled: checked })}
          />
          <FieldLabel htmlFor="channel-disabled">Start disabled</FieldLabel>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Creating…" : "Create sales channel"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/sales-channels" />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  )
}
