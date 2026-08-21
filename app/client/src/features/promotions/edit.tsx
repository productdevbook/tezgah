import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import { promotion, updatePromotion, type Promotion } from "@/api/schemas"
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

/**
 * A limit is a whole number or nothing at all, and an input holds a string
 * either way — so it is collected as one and turned into a number on submit,
 * where `""` means "no limit" rather than zero.
 */
const limit = z
  .string()
  .trim()
  .refine(
    (value) => value === "" || /^\d+$/.test(value),
    "a whole number, or nothing for no limit"
  )

const fields = z.object({
  code: z.string().trim().min(1, "a code is needed"),
  is_automatic: z.boolean(),
  usage_limit: limit,
  customer_usage_limit: limit,
})

type Fields = z.infer<typeof fields>

const orNoLimit = (value: string) =>
  value.trim() === "" ? null : Number(value)

export function EditPromotion({ id }: { id: string }) {
  const t = useT()
  const navigate = useNavigate()
  const result = useDetail(
    ["promotions"],
    "/admin/promotions/{id}",
    promotion,
    id
  )

  return (
    <RouteDrawer
      onClose={() => void navigate({ to: "/promotions/$id", params: { id } })}
    >
      <RouteDrawer.Header title={t("form.promotion.title")} />
      {result.data ? (
        <Body item={result.data} />
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{ title: "No promotion", description: "Nothing to show." }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}

function Body({ item }: { item: Promotion }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      code: item.code,
      is_automatic: item.is_automatic,
      usage_limit: item.usage_limit === null ? "" : String(item.usage_limit),
      customer_usage_limit:
        item.customer_usage_limit === null
          ? ""
          : String(item.customer_usage_limit),
    },
  })

  const mutation = useMutation({
    mutationFn: (body: z.input<typeof updatePromotion>) =>
      patch("/admin/promotions/{id}", {
        schema: promotion,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["promotions"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          code: values.code,
          is_automatic: values.is_automatic,
          usage_limit: orNoLimit(values.usage_limit),
          customer_usage_limit: orNoLimit(values.customer_usage_limit),
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="code" label={t("field.code")}>
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="is_automatic"
            label={t("form.promotion.automatic")}
            description={t("form.promotion.automaticWhy")}
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
            name="usage_limit"
            label={t("form.promotion.usesTotal")}
            description={t("form.promotion.usesTotalWhy")}
          >
            {(field) => (
              <Input id={field.name} inputMode="numeric" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="customer_usage_limit"
            label={t("form.promotion.usesPerCustomer")}
            description={t("form.promotion.noLimit")}
          >
            {(field) => (
              <Input id={field.name} inputMode="numeric" {...field} />
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
