import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useForm, useWatch } from "react-hook-form"
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
  thumbnail_url: z
    .string()
    .trim()
    .refine(
      (value) => value === "" || /^https?:\/\//.test(value),
      "an http or https address, or nothing"
    ),
})

type Fields = z.infer<typeof fields>

/**
 * A URL and no upload, and that is the library's decision rather than this
 * screen's: tezgah stores no files. A host already has media, so a product
 * carries the address of an image somebody else is serving —
 * `docs/architecture.md` has it under what a host supplies.
 */
export function EditMedia({ id }: { id: string }) {
  return (
    <ProductDrawer
      id={id}
      title="Media"
      description="An address, not an upload. tezgah stores no files — whatever already serves the shop's images serves this one."
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
    defaultValues: { thumbnail_url: item.thumbnail_url ?? "" },
  })

  // `useWatch` rather than `form.watch()`: the second cannot be memoized, and
  // the lint that says so is right — it re-renders this on every keystroke of
  // every field rather than of this one.
  const url = useWatch({ control: form.control, name: "thumbnail_url" })

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
          thumbnail_url: values.thumbnail_url || null,
        })
      }
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField
            control={form.control}
            name="thumbnail_url"
            label="Thumbnail"
          >
            {(field) => (
              <Input id={field.name} placeholder="https://…" {...field} />
            )}
          </FormField>
          {/* Shown rather than described: an address that 404s is a broken
              image here, which is the fastest way to find out. */}
          {url && /^https?:\/\//.test(url) ? (
            <img
              src={url}
              alt=""
              className="max-h-48 w-full rounded-md bg-muted object-contain"
            />
          ) : null}
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
