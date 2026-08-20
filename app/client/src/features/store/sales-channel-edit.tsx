import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import {
  salesChannel,
  updateSalesChannel,
  type SalesChannel,
} from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

const fields = z.object({
  name: z.string().trim().min(1, "a name is needed"),
  description: z.string().trim(),
  is_disabled: z.boolean(),
})

type Fields = z.infer<typeof fields>

export function EditSalesChannel({ id }: { id: string }) {
  const navigate = useNavigate()
  const result = useDetail(
    ["sales-channels"],
    "/admin/sales-channels/{id}",
    salesChannel,
    id
  )

  return (
    <RouteDrawer
      onClose={() =>
        void navigate({ to: "/store/sales-channels/$id", params: { id } })
      }
    >
      <RouteDrawer.Header title="Edit sales channel" />
      {result.data ? (
        <Body item={result.data} />
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{
              title: "No sales channel",
              description: "Nothing to show.",
            }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}

function Body({ item }: { item: SalesChannel }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      name: item.name,
      description: item.description ?? "",
      is_disabled: item.is_disabled,
    },
  })

  const mutation = useMutation({
    mutationFn: (body: z.input<typeof updateSalesChannel>) =>
      patch("/admin/sales-channels/{id}", {
        schema: salesChannel,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["sales-channels"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          name: values.name,
          description:
            values.description.trim() === "" ? null : values.description,
          is_disabled: values.is_disabled,
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="name" label="Name">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="description"
            label="Description"
          >
            {(field) => <Textarea id={field.name} {...field} rows={4} />}
          </FormField>
          <FormField
            control={form.control}
            name="is_disabled"
            label="Disabled"
            description="A disabled channel keeps its products and stops selling them."
          >
            {(field) => (
              <Switch
                id={field.name}
                checked={field.value}
                onCheckedChange={(checked) => field.onChange(checked)}
              />
            )}
          </FormField>
        </div>
      </RouteDrawer.Body>
      <RouteDrawer.Footer>
        <Button type="button" variant="outline" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button type="submit" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting
            ? t("actions.saving")
            : t("actions.save")}
        </Button>
      </RouteDrawer.Footer>
    </RouteModalForm>
  )
}
