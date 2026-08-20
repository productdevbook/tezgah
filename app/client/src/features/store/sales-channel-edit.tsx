import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { patch } from "@/api/client"
import {
  salesChannel,
  updateSalesChannel,
  type SalesChannel,
  type UpdateSalesChannel,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useDetail } from "@/lib/detail"

/** `/store/sales-channels/$id/edit` — the trailing-underscore escape
 * `sales-channel-detail.tsx` already takes, for the same reason. */
export function EditSalesChannel({ id }: { id: string }) {
  const result = useDetail(
    ["sales-channels"],
    "/admin/sales-channels/{id}",
    salesChannel,
    id
  )

  return (
    <RouteDrawer>
      <RouteDrawer.Header title="Edit sales channel" />
      <RouteDrawer.Body>
      <QueryState
        query={result}
        empty={{ title: "No sales channel", description: "Nothing to show." }}
      >
        {(item) => <SalesChannelForm item={item} />}
      </QueryState>
    </RouteDrawer.Body>
    </RouteDrawer>
  )
}

function SalesChannelForm({ item }: { item: SalesChannel }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({
    name: item.name,
    description: item.description ?? "",
    is_disabled: item.is_disabled,
  })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: UpdateSalesChannel) =>
      patch("/admin/sales-channels/{id}", {
        schema: salesChannel,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["sales-channels"] })
      void navigate({ to: "/store/sales-channels/$id", params: { id: item.id } })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = updateSalesChannel.safeParse({
      name: form.name,
      description: form.description.trim() === "" ? null : form.description,
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
    <>
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="channel-name">Name</FieldLabel>
          <Input
            id="channel-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
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
          <FieldLabel htmlFor="channel-disabled">Disabled</FieldLabel>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/sales-channels/$id" params={{ id: item.id }} />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </>
  )
}
