import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import { region, updateRegion, type Region } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import { useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

const fields = z.object({
  name: z.string().trim().min(1, "a name is needed"),
  currency_code: z
    .string()
    .trim()
    .length(3, "a currency code is three letters"),
  is_tax_inclusive: z.boolean(),
  has_automatic_taxes: z.boolean(),
})

type Fields = z.infer<typeof fields>

/**
 * `tezgah::api` has no route to delete a region, only one to take a country
 * out of it — so this screen, unlike the other four, offers no delete.
 */
export function EditRegion({ id }: { id: string }) {
  const navigate = useNavigate()
  const result = useDetail(["regions"], "/admin/regions/{id}", region, id)

  return (
    <RouteDrawer
      onClose={() =>
        void navigate({ to: "/store/regions/$id", params: { id } })
      }
    >
      <RouteDrawer.Header title="Edit region" />
      {result.data ? (
        <Body item={result.data} />
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{ title: "No region", description: "Nothing to show." }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}

function Body({ item }: { item: Region }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      name: item.name,
      currency_code: item.currency_code,
      is_tax_inclusive: item.is_tax_inclusive,
      has_automatic_taxes: item.has_automatic_taxes,
    },
  })

  const mutation = useMutation({
    mutationFn: (body: z.input<typeof updateRegion>) =>
      patch("/admin/regions/{id}", {
        schema: region,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["regions"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) => mutation.mutateAsync(values)}
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="name" label="Name">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="currency_code"
            label="Currency code"
          >
            {(field) => (
              <Input id={field.name} className="uppercase" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="is_tax_inclusive"
            label="Prices include tax"
            description="What a shopper here is shown: a price with tax already in it, or one that gains tax at the till."
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
            label="Work tax out automatically"
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
