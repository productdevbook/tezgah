import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import { customer, updateCustomer, type Customer } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

/**
 * What the form collects: a text input holds `""` and never `null`, and the
 * API wants `null` to mean "clear it". The conversion lives in `onSubmit`.
 */
const fields = z.object({
  email: z.string().trim(),
  first_name: z.string().trim(),
  last_name: z.string().trim(),
  phone: z.string().trim(),
  company_name: z.string().trim(),
})

type Fields = z.infer<typeof fields>

const orNull = (value: string) => (value.trim() === "" ? null : value)

export function EditCustomer({ id }: { id: string }) {
  const t = useT()
  const navigate = useNavigate()
  const result = useDetail(["customers"], "/admin/customers/{id}", customer, id)

  return (
    <RouteDrawer
      onClose={() => void navigate({ to: "/customers/$id", params: { id } })}
    >
      <RouteDrawer.Header title={t("form.customer.edit")} />
      {result.data ? (
        <Body item={result.data} />
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{
              title: t("empty.customer"),
              description: t("general.nothingToShow"),
            }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}

function Body({ item }: { item: Customer }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      email: item.email ?? "",
      first_name: item.first_name ?? "",
      last_name: item.last_name ?? "",
      phone: item.phone ?? "",
      company_name: item.company_name ?? "",
    },
  })

  const mutation = useMutation({
    mutationFn: (body: z.input<typeof updateCustomer>) =>
      patch("/admin/customers/{id}", {
        schema: customer,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["customers"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          email: orNull(values.email),
          first_name: orNull(values.first_name),
          last_name: orNull(values.last_name),
          phone: orNull(values.phone),
          company_name: orNull(values.company_name),
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField
            control={form.control}
            name="email"
            label={t("field.email")}
          >
            {(field) => <Input id={field.name} type="email" {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="first_name"
            label={t("field.firstName")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="last_name"
            label={t("field.lastName")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="phone"
            label={t("field.phone")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="company_name"
            label={t("field.company")}
          >
            {(field) => <Input id={field.name} {...field} />}
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
