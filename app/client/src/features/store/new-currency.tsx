import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { post } from "@/api/client"
import { createCurrency, currency, type CreateCurrency } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useT } from "@/panel/i18n"

/**
 * The exponent is a whole number in an input that holds a string, and
 * `numeric_code` is optional — so what the form collects is not what the API
 * takes, and the conversion is in `onSubmit` rather than spread through the
 * fields.
 */
const fields = createCurrency
  .omit({ exponent: true, numeric_code: true })
  .extend({
    numeric_code: z.string().trim(),
    exponent: z
      .string()
      .trim()
      .refine(
        (value) => /^[0-4]$/.test(value),
        "a currency's exponent is between 0 and 4"
      ),
  })

type Fields = z.infer<typeof fields>

export function NewCurrency() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal onClose={() => void navigate({ to: "/store/currencies" })}>
      <Body />
    </RouteFocusModal>
  )
}

function Body() {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      code: "",
      numeric_code: "",
      exponent: "2",
      symbol: "",
      symbol_native: "",
      name: "",
    },
  })

  const mutation = useMutation({
    mutationFn: (body: CreateCurrency) =>
      post("/admin/currencies", { schema: currency, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["currencies"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          code: values.code,
          numeric_code:
            values.numeric_code.trim() === "" ? undefined : values.numeric_code,
          exponent: Number(values.exponent),
          symbol: values.symbol,
          symbol_native: values.symbol_native,
          name: values.name,
        })
      }
    >
      <RouteFocusModal.Header
        title={t("form.currency.title")}
        description={t("form.currency.why")}
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="code" label={t("field.code")}>
            {(field) => (
              <Input
                id={field.name}
                className="uppercase"
                placeholder="TRY"
                {...field}
              />
            )}
          </FormField>
          <FormField control={form.control} name="name" label={t("field.name")}>
            {(field) => (
              <Input id={field.name} placeholder="Turkish lira" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="symbol"
            label={t("field.symbol")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="symbol_native"
            label={t("field.nativeSymbol")}
            description={t("form.currency.nativeWhy")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="exponent"
            label={t("field.exponent")}
            description={t("form.currency.exponentWhy")}
          >
            {(field) => (
              <Input id={field.name} inputMode="numeric" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="numeric_code"
            label={t("field.numericCode")}
            description={t("form.currency.numericWhy")}
          >
            {(field) => (
              <Input id={field.name} inputMode="numeric" {...field} />
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
