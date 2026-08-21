import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { patch } from "@/api/client"
import { product, updateProduct, type Product } from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"
import {
  orNull,
  productFields,
  type ProductFields,
} from "@/features/products/fields"

/**
 * `/products/$id/edit` — a drawer over the product's own page, so what is
 * being changed is still readable behind the form.
 *
 * It was a full page until now, and unreachable: `/products/$id/edit` is a
 * child route of `/products/$id`, and that page rendered no `<Outlet />`, so
 * the address resolved and nothing was ever drawn. Four other sections had
 * the same hole.
 *
 * Loads the record itself rather than trusting what the list row carried, so
 * the form opens with what the server holds right now. `PATCH
 * /admin/products/{id}` has no `status` field — publishing is not something
 * this route does — so this form does not offer one.
 */
export function EditProduct({ id }: { id: string }) {
  const t = useT()
  const navigate = useNavigate()
  const result = useDetail(["products"], "/admin/products/{id}", product, id)

  return (
    <RouteDrawer
      onClose={() => void navigate({ to: "/products/$id", params: { id } })}
    >
      <RouteDrawer.Header title={t("form.product.edit")} />
      {result.data ? (
        // The form is built from what came back, so it cannot be rendered
        // before there is anything to build it from — and its own body and
        // footer are the drawer's, which is why the loading state gets a
        // body of its own rather than sitting inside the form's.
        <Body item={result.data} />
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{ title: "No product", description: "Nothing to show." }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}

function Body({ item }: { item: Product }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const form = useForm<ProductFields>({
    resolver: zodResolver(productFields),
    defaultValues: {
      handle: item.handle,
      title: item.title,
      subtitle: item.subtitle ?? "",
      description: item.description ?? "",
    },
  })

  // `z.input`, not `z.infer`: the generated schema gives
  // `product_collection_id` and `product_type_id` a default of `null`, so the
  // parsed *output* type demands both — and the Rust reads them through
  // `double_option`, where an explicit null clears the field. Building a
  // PATCH from the output type therefore detaches a product from its
  // collection and type on every save.
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
          handle: values.handle,
          title: values.title,
          subtitle: orNull(values.subtitle),
          description: orNull(values.description),
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField
            control={form.control}
            name="handle"
            label={t("field.handle")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="title"
            label={t("field.title")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="subtitle"
            label={t("field.subtitle")}
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="description"
            label={t("field.description")}
          >
            {(field) => <Textarea id={field.name} {...field} rows={5} />}
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
