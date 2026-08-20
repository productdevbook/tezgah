import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { post } from "@/api/client"
import {
  createSalesChannel,
  salesChannel,
  type CreateSalesChannel,
} from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { Textarea } from "@/components/ui/textarea"
import { useT } from "@/panel/i18n"

const fields = createSalesChannel
  .omit({ description: true })
  .extend({ description: z.string().trim() })

type Fields = z.infer<typeof fields>

export function NewSalesChannel() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal
      onClose={() => void navigate({ to: "/store/sales-channels" })}
    >
      <Body />
    </RouteFocusModal>
  )
}

function Body() {
  const t = useT()
  const client = useQueryClient()
  const navigate = useNavigate()
  const { close, markSaved } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: { name: "", description: "", is_disabled: false },
  })

  const mutation = useMutation({
    mutationFn: (body: CreateSalesChannel) =>
      post("/admin/sales-channels", { schema: salesChannel, body }),
    onSuccess: (created) => {
      void client.invalidateQueries({ queryKey: ["sales-channels"] })
      markSaved()
      void navigate({
        to: "/store/sales-channels/$id",
        params: { id: created.id },
      })
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          name: values.name,
          description:
            values.description.trim() === "" ? undefined : values.description,
          is_disabled: values.is_disabled,
        })
      }
    >
      <RouteFocusModal.Header
        title="New sales channel"
        description="Where a product is sold: a web shop, an app, a market stall. A product belongs to some of them and not others."
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
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
      </RouteFocusModal.Body>
      <RouteFocusModal.Footer>
        <Button type="button" variant="outline" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button type="submit" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting
            ? t("actions.saving")
            : t("actions.create")}
        </Button>
      </RouteFocusModal.Footer>
    </RouteModalForm>
  )
}
