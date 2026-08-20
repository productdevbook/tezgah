import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { patch } from "@/api/client"
import { product, updateProduct, type Product, type UpdateProduct } from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { useDetail } from "@/lib/detail"

/**
 * `/products/$id/edit` — its own address, the same as `/products/new`, so
 * cancelling is a navigation and the back button lands on the detail page it
 * was opened from. Loads the record itself (`useDetail`, the same single
 * read the detail page uses) rather than trusting whatever the list row
 * carried, so the form opens with what the server holds right now.
 *
 * `PATCH /admin/products/{id}` has no `status` field — publishing a product
 * is not something this route does — so this form does not offer one.
 */
export function EditProduct({ id }: { id: string }) {
  const result = useDetail(["products"], "/admin/products/{id}", product, id)

  return (
    <div className="max-w-xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No product", description: "Nothing to show." }}
      >
        {(item) => <ProductForm item={item} />}
      </QueryState>
    </div>
  )
}

function ProductForm({ item }: { item: Product }) {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState({
    handle: item.handle,
    title: item.title,
    subtitle: item.subtitle ?? "",
    description: item.description ?? "",
  })
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: UpdateProduct) =>
      patch("/admin/products/{id}", { schema: product, params: { id: item.id }, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["products"] })
      void navigate({ to: "/products/$id", params: { id: item.id } })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = updateProduct.safeParse({
      handle: form.handle,
      title: form.title,
      subtitle: form.subtitle.trim() === "" ? null : form.subtitle,
      description: form.description.trim() === "" ? null : form.description,
    })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <>
      <PageHeading title={`Edit ${item.title}`} subtitle={item.handle} />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.handle}>
          <FieldLabel htmlFor="product-handle">Handle</FieldLabel>
          <Input
            id="product-handle"
            value={form.handle}
            onChange={(e) => setForm({ ...form, handle: e.target.value })}
            aria-invalid={!!fieldErrors.handle}
          />
          <FieldError>{fieldErrors.handle}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.title}>
          <FieldLabel htmlFor="product-title">Title</FieldLabel>
          <Input
            id="product-title"
            value={form.title}
            onChange={(e) => setForm({ ...form, title: e.target.value })}
            aria-invalid={!!fieldErrors.title}
          />
          <FieldError>{fieldErrors.title}</FieldError>
        </Field>
        <Field>
          <FieldLabel htmlFor="product-subtitle">Subtitle</FieldLabel>
          <Input
            id="product-subtitle"
            value={form.subtitle}
            onChange={(e) => setForm({ ...form, subtitle: e.target.value })}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor="product-description">Description</FieldLabel>
          <Textarea
            id="product-description"
            value={form.description}
            onChange={(e) => setForm({ ...form, description: e.target.value })}
          />
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Saving…" : "Save"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/products/$id" params={{ id: item.id }} />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </>
  )
}
