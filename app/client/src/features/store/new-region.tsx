import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import type { z } from "zod"

import { post } from "@/api/client"
import { createRegion, region, type CreateRegion } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useT } from "@/panel/i18n"

type Fields = z.infer<typeof createRegion>

export function NewRegion() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal onClose={() => void navigate({ to: "/store/regions" })}>
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
    resolver: zodResolver(createRegion),
    defaultValues: {
      name: "",
      currency_code: "",
      is_tax_inclusive: false,
      has_automatic_taxes: true,
    },
  })

  const mutation = useMutation({
    mutationFn: (body: CreateRegion) =>
      post("/admin/regions", { schema: region, body }),
    onSuccess: (created) => {
      void client.invalidateQueries({ queryKey: ["regions"] })
      markSaved()
      void navigate({ to: "/store/regions/$id", params: { id: created.id } })
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) => mutation.mutateAsync(values)}
    >
      <RouteFocusModal.Header
        title={t("form.region.new")}
        description={t("form.region.why")}
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="name" label={t("field.name")}>
            {(field) => (
              <Input id={field.name} placeholder="Türkiye" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="currency_code"
            label={t("field.currencyCode")}
            description={t("form.region.currencyWhy")}
          >
            {(field) => (
              <Input
                id={field.name}
                className="uppercase"
                placeholder="TRY"
                {...field}
              />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="is_tax_inclusive"
            label={t("field.pricesIncludeTax")}
          >
            {(field) => (
              <Switch
                id={field.name}
                checked={field.value}
                onCheckedChange={(checked) => field.onChange(checked)}
              />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="has_automatic_taxes"
            label={t("field.autoTax")}
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
