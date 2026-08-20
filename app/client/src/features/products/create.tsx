import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useNavigate } from "@tanstack/react-router"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { post } from "@/api/client"
import {
  product,
  productStatus,
  type CreateProduct as CreateProductBody,
  type ProductStatus,
} from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { useRouteModal } from "@/components/modals/route-modal"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { useT } from "@/panel/i18n"
import {
  EMPTY_PRODUCT,
  orAbsent,
  productFields,
  type ProductFields,
} from "@/features/products/fields"

/** The create form's own shape: the four fields plus a status the API lets
 * default. `""` is "leave it to the server", which is not a status the API
 * has a name for. */
const createFields = productFields.extend({
  status: z.union([productStatus, z.literal("")]),
})

type Form = ProductFields & { status: ProductStatus | "" }

/**
 * `/products/new` — still an address, still linkable, but drawn over the list
 * it came from rather than replacing it. An operator who opens it has not
 * lost the page of products they were looking at, and cancelling puts them
 * back on it with the cursor they had.
 */
export function CreateProduct() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal onClose={() => void navigate({ to: "/products" })}>
      <Body />
    </RouteFocusModal>
  )
}

function Body() {
  const t = useT()
  const client = useQueryClient()
  const navigate = useNavigate()
  const { close, markSaved } = useRouteModal()

  const form = useForm<Form>({
    resolver: zodResolver(createFields),
    defaultValues: { ...EMPTY_PRODUCT, status: "" },
  })

  const mutation = useMutation({
    mutationFn: (body: CreateProductBody) =>
      post("/admin/products", { schema: product, body }),
    onSuccess: (created) => {
      void client.invalidateQueries({ queryKey: ["products"] })
      markSaved()
      void navigate({ to: "/products/$id", params: { id: created.id } })
    },
  })

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) =>
        mutation.mutateAsync({
          handle: values.handle,
          title: values.title,
          subtitle: orAbsent(values.subtitle),
          description: orAbsent(values.description),
          status: values.status === "" ? undefined : values.status,
        })
      }
    >
      <RouteFocusModal.Header
        title="New product"
        description="Starts as a draft. Variants, prices and stock go in separately."
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField control={form.control} name="handle" label="Handle">
            {(field) => <Input id={field.name} {...field} placeholder="denim-jacket" />}
          </FormField>
          <FormField control={form.control} name="title" label="Title">
            {(field) => <Input id={field.name} {...field} placeholder="Denim jacket" />}
          </FormField>
          <FormField control={form.control} name="subtitle" label="Subtitle">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="description"
            label="Description"
          >
            {(field) => <Textarea id={field.name} {...field} rows={5} />}
          </FormField>
          <FormField control={form.control} name="status" label="Status">
            {(field) => (
              <Select
                value={field.value || undefined}
                onValueChange={(value) => field.onChange(value)}
              >
                <SelectTrigger id={field.name}>
                  <SelectValue placeholder="draft (default)" />
                </SelectTrigger>
                <SelectContent>
                  {productStatus.options.map((option) => (
                    <SelectItem key={option} value={option}>
                      {option}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
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
