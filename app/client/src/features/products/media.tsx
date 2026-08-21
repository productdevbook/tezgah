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
import { UploadImage } from "@/features/products/upload"

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
 * A URL, and an upload when the host has somewhere to put one.
 *
 * The column holds an address either way, which is the library's decision
 * rather than this screen's: tezgah stores no files. What changed is that
 * `app/server` can be one of the hosts that does — started with a file
 * directory, it takes the image and hands back the address; started without,
 * the upload route is not bound and this falls back to what it always was.
 */
export function EditMedia({ id }: { id: string }) {
  const t = useT()
  return (
    <ProductDrawer
      id={id}
      title={t("section.media")}
      description={t("section.mediaWhy")}
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
            label={t("field.thumbnail")}
          >
            {(field) => (
              <Input id={field.name} placeholder="https://…" {...field} />
            )}
          </FormField>
          {/* Writes into the same field rather than saving by itself: the
              upload stored a file, and which product points at it is still
              this form's save. */}
          <UploadImage
            onStored={(stored) =>
              form.setValue("thumbnail_url", stored, {
                shouldDirty: true,
                shouldValidate: true,
              })
            }
          />
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
