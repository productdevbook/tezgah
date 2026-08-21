import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import { product, updateProduct, type Product } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useT } from "@/panel/i18n"
import { ProductDrawer } from "@/features/products/drawer"

/**
 * A measurement is a number in a box that holds a string, and an empty box
 * means "there is no such measurement" rather than zero — a product with a
 * weight of nothing is not a weightless product.
 */
const measure = z
  .string()
  .trim()
  .refine(
    (value) => value === "" || /^\d+(\.\d+)?$/.test(value),
    "a number, or nothing"
  )

const fields = z.object({
  weight: measure,
  length: measure,
  height: measure,
  width: measure,
  material: z.string().trim(),
  hs_code: z.string().trim(),
  origin_country: z
    .string()
    .trim()
    .refine(
      (value) => value === "" || /^[A-Za-z]{2}$/.test(value),
      "two letters, or nothing"
    ),
})

type Fields = z.infer<typeof fields>

const orNull = (value: string) => (value.trim() === "" ? null : value)

export function EditAttributes({ id }: { id: string }) {
  const t = useT()
  return (
    <ProductDrawer
      id={id}
      title={t("form.attributes.title")}
      description={t("form.attributes.why")}
    >
      {(item) => <Body item={item} />}
    </ProductDrawer>
  )
}

function Body({ item }: { item: Product }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const asText = (value: string | number | null | undefined) =>
    value === null || value === undefined ? "" : String(value)

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      weight: asText(item.weight),
      length: asText(item.length),
      height: asText(item.height),
      width: asText(item.width),
      material: item.material ?? "",
      hs_code: item.hs_code ?? "",
      origin_country: item.origin_country ?? "",
    },
  })

  const mutation = useMutation({
    mutationFn: (body: z.input<typeof updateProduct>) =>
      patch("/admin/products/{id}", {
        schema: product,
        params: { id: item.id },
        body,
      }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["products"] })
      succeed()
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          weight: orNull(values.weight),
          length: orNull(values.length),
          height: orNull(values.height),
          width: orNull(values.width),
          material: orNull(values.material),
          hs_code: orNull(values.hs_code),
          origin_country: orNull(values.origin_country),
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <div className="grid grid-cols-2 gap-4">
            <FormField
              control={form.control}
              name="weight"
              label={t("field.weight")}
            >
              {(field) => (
                <Input id={field.name} inputMode="decimal" {...field} />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="length"
              label={t("field.length")}
            >
              {(field) => (
                <Input id={field.name} inputMode="decimal" {...field} />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="height"
              label={t("field.height")}
            >
              {(field) => (
                <Input id={field.name} inputMode="decimal" {...field} />
              )}
            </FormField>
            <FormField
              control={form.control}
              name="width"
              label={t("field.width")}
            >
              {(field) => (
                <Input id={field.name} inputMode="decimal" {...field} />
              )}
            </FormField>
          </div>
          <FormField
            control={form.control}
            name="material"
            label={t("field.material")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="hs_code"
            label={t("field.hsCode")}
            description={t("form.attributes.hsWhy")}
          >
            {(field) => (
              <Input id={field.name} className="font-mono text-xs" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="origin_country"
            label={t("field.originCountry")}
            description={t("form.attributes.originWhy")}
          >
            {(field) => (
              <Input id={field.name} className="uppercase" {...field} />
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
