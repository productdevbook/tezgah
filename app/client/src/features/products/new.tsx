import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"

import { post } from "@/api/client"
import {
  createProduct,
  product,
  productStatus,
  type CreateProduct,
  type ProductStatus,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"

const EMPTY_FORM = {
  handle: "",
  title: "",
  subtitle: "",
  description: "",
  status: "" as ProductStatus | "",
}

/**
 * `/products/new` — its own address rather than a dialog over the list, so
 * cancelling or saving is just a navigation and the back button works.
 */
export function NewProduct() {
  const client = useQueryClient()
  const navigate = useNavigate()
  const [form, setForm] = useState(EMPTY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateProduct) =>
      post("/admin/products", { schema: product, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["products"] })
      void navigate({ to: "/products" })
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createProduct.safeParse({
      handle: form.handle,
      title: form.title,
      subtitle: form.subtitle.trim() === "" ? undefined : form.subtitle,
      description:
        form.description.trim() === "" ? undefined : form.description,
      status: form.status === "" ? undefined : form.status,
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
    <div className="max-w-xl space-y-4">
      <PageHeading
        title="New product"
        subtitle="Starts as a draft. Variants, prices and stock go in separately."
      />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.handle}>
          <FieldLabel htmlFor="product-handle">Handle</FieldLabel>
          <Input
            id="product-handle"
            value={form.handle}
            onChange={(e) => setForm({ ...form, handle: e.target.value })}
            placeholder="denim-jacket"
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
            placeholder="Denim jacket"
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
        <Field>
          <FieldLabel htmlFor="product-status">Status</FieldLabel>
          <Select
            value={form.status || undefined}
            onValueChange={(v) => setForm({ ...form, status: v as ProductStatus })}
          >
            <SelectTrigger id="product-status">
              <SelectValue placeholder="draft (default)" />
            </SelectTrigger>
            <SelectContent>
              {productStatus.options.map((s) => (
                <SelectItem key={s} value={s}>
                  {s}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Creating…" : "Create product"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/products" />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  )
}
