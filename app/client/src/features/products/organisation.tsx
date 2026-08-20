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

const fields = z.object({
  product_type_id: z
    .string()
    .trim()
    .refine(
      (v) => v === "" || z.uuid().safeParse(v).success,
      "that is not an id"
    ),
  product_collection_id: z
    .string()
    .trim()
    .refine(
      (v) => v === "" || z.uuid().safeParse(v).success,
      "that is not an id"
    ),
  external_id: z.string().trim(),
})

type Fields = z.infer<typeof fields>

export function EditOrganisation({ id }: { id: string }) {
  return (
    <ProductDrawer
      id={id}
      title="Organisation"
      description="What this product belongs to, and what it is called in whatever system it came from."
    >
      {(item) => <Body item={item} />}
    </ProductDrawer>
  )
}

function Body({ item }: { item: Product }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      product_type_id: item.product_type_id ?? "",
      product_collection_id: item.product_collection_id ?? "",
      external_id: item.external_id ?? "",
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
        // These two are the fields `UpdateProduct` reads through
        // `double_option`, where absent means "leave it" and an explicit null
        // means "clear it". This form owns them, so an empty box is a clear
        // rather than a no-op — which is why it sends null instead of leaving
        // the field out.
        mutation.mutateAsync({
          product_type_id: values.product_type_id || null,
          product_collection_id: values.product_collection_id || null,
          external_id: values.external_id || null,
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField
            control={form.control}
            name="product_type_id"
            label="Product type"
            description="An id. Empty clears it."
          >
            {(field) => (
              <Input id={field.name} className="font-mono text-xs" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="product_collection_id"
            label="Collection"
            description="An id. Empty clears it."
          >
            {(field) => (
              <Input id={field.name} className="font-mono text-xs" {...field} />
            )}
          </FormField>
          <FormField
            control={form.control}
            name="external_id"
            label="External ID"
            description="What this product is called wherever it came from."
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
