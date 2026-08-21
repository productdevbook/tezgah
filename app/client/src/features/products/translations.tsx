import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useForm } from "react-hook-form"
import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { LOCALES, useT } from "@/panel/i18n"
import { ProductDrawer } from "@/features/products/drawer"

/**
 * Transcribed from `admin_catalogue::TranslationView` and `PutTranslation`.
 *
 * A locale is a free string in the crate — tezgah does not decide what
 * languages a shop sells in, and a list of the ones this panel can draw
 * itself in would be the wrong list.
 */
const translation = z.object({
  product_id: z.string(),
  locale: z.string(),
  title: z.string(),
  subtitle: z.string().nullable(),
  description: z.string().nullable(),
  handle: z.string().nullable(),
})

type Translation = z.infer<typeof translation>

const fields = z.object({
  locale: z.string().trim().min(2, "a locale like `tr` or `tr-TR`"),
  title: z.string().trim().min(1, "a title is what a shopper reads"),
  subtitle: z.string(),
  description: z.string(),
  handle: z.string(),
})

type Fields = z.infer<typeof fields>

async function list(productId: string): Promise<Translation[]> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    `/admin/products/${encodeURIComponent(productId)}/translations`
  )
  return parseResponse(z.array(translation), data, status)
}

async function put(productId: string, body: unknown): Promise<void> {
  await apiMutator(
    `/admin/products/${encodeURIComponent(productId)}/translations`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }
  )
}

/**
 * What a product is called in another language.
 *
 * One locale at a time, written whole: the route takes a title and three
 * optional fields, and a form that saved one field at a time would be four
 * writes where the crate offers one.
 *
 * The panel's own two languages are offered as a shortcut and not as a limit
 * — a shop selling in a language this panel has never been translated into
 * types the code, because what a shop sells in and what its back office is
 * drawn in are different questions.
 */
export function EditTranslations({ id }: { id: string }) {
  return (
    <ProductDrawer
      id={id}
      title="Translations"
      description="What this product is called in another language. The storefront asks for one and falls back to the shop's own."
    >
      {() => <Body productId={id} />}
    </ProductDrawer>
  )
}

function Body({ productId }: { productId: string }) {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed } = useRouteModal()

  const existing = useQuery({
    queryKey: ["product-translations", productId],
    queryFn: () => list(productId),
  })

  const form = useForm<Fields>({
    resolver: zodResolver(fields),
    defaultValues: {
      locale: "",
      title: "",
      subtitle: "",
      description: "",
      handle: "",
    },
  })

  const mutation = useMutation({
    mutationFn: (values: Fields) =>
      put(productId, {
        locale: values.locale.trim(),
        title: values.title.trim(),
        // Empty is nothing rather than an empty string: the column is
        // nullable, and a blank subtitle is the absence of one.
        subtitle: values.subtitle.trim() || null,
        description: values.description.trim() || null,
        handle: values.handle.trim() || null,
      }),
    onSuccess: () => {
      void client.invalidateQueries({
        queryKey: ["product-translations", productId],
      })
      succeed()
    },
  })

  const written = existing.data ?? []

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) => mutation.mutateAsync(values)}
    >
      <RouteDrawer.Body>
        <div className="flex flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}

          {written.length > 0 ? (
            <div className="flex flex-wrap gap-2">
              {written.map((one) => (
                <Badge
                  key={one.locale}
                  variant="outline"
                  className="cursor-pointer"
                  onClick={() =>
                    form.reset({
                      locale: one.locale,
                      title: one.title,
                      subtitle: one.subtitle ?? "",
                      description: one.description ?? "",
                      handle: one.handle ?? "",
                    })
                  }
                >
                  {one.locale}
                </Badge>
              ))}
            </div>
          ) : (
            <p className="text-sm text-muted-foreground">
              None yet. A storefront asking for a language this product has not
              been translated into gets the shop's own words.
            </p>
          )}

          <FormField control={form.control} name="locale" label="Locale">
            {(field) => (
              <div className="flex gap-2">
                <Input
                  id={field.name}
                  placeholder="tr, tr-TR, de…"
                  {...field}
                />
                <Select
                  value=""
                  onValueChange={(value) => field.onChange(value)}
                >
                  <SelectTrigger className="w-28" size="sm">
                    <SelectValue placeholder="or…" />
                  </SelectTrigger>
                  <SelectContent>
                    {Object.keys(LOCALES).map((one) => (
                      <SelectItem key={one} value={one}>
                        {one}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}
          </FormField>
          <FormField control={form.control} name="title" label="Title">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField control={form.control} name="subtitle" label="Subtitle">
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField
            control={form.control}
            name="description"
            label="Description"
          >
            {(field) => <Input id={field.name} {...field} />}
          </FormField>
          <FormField control={form.control} name="handle" label="Handle">
            {(field) => (
              <Input
                id={field.name}
                placeholder="left empty, the shop's own"
                {...field}
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
