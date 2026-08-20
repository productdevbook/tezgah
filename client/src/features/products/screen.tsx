import { useState, type FormEvent } from "react"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"

import { post } from "@/api/client"
import {
  createProduct,
  product,
  productStatus,
  type CreateProduct,
  type Product,
  type ProductStatus,
} from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { FormError } from "@/components/form-error"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
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
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"

/** Four of the five mean "not for sale", for four different reasons. */
const HIDDEN: ProductStatus[] = ["draft", "proposed", "rejected", "archived"]

const columns: Columns<Product> = [
  {
    header: "Title",
    accessorKey: "title",
    cell: ({ row }) => (
      <div className="min-w-0">
        <div className="truncate font-medium">{row.original.title}</div>
        {row.original.subtitle ? (
          <div className="truncate text-xs text-muted-foreground">
            {row.original.subtitle}
          </div>
        ) : null}
      </div>
    ),
  },
  {
    header: "Handle",
    accessorKey: "handle",
    meta: { className: "text-muted-foreground font-mono text-xs" },
  },
  {
    header: "Status",
    accessorKey: "status",
    cell: ({ row }) => (
      <Badge
        variant={HIDDEN.includes(row.original.status) ? "outline" : "default"}
      >
        {row.original.status}
      </Badge>
    ),
  },
  {
    header: "Discountable",
    accessorKey: "is_discountable",
    cell: ({ row }) => (row.original.is_discountable ? "yes" : "no"),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

const EMPTY_FORM = {
  handle: "",
  title: "",
  subtitle: "",
  description: "",
  status: "" as ProductStatus | "",
}

function NewProductDialog() {
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateProduct) =>
      post("/admin/products", { schema: product, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["products"] })
      setOpen(false)
      setForm(EMPTY_FORM)
      setFieldErrors({})
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
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setForm(EMPTY_FORM)
          setFieldErrors({})
          mutation.reset()
        }
      }}
    >
      <DialogTrigger render={<Button size="sm" />}>
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        New product
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New product</DialogTitle>
          <DialogDescription>
            Starts as a draft. Variants, prices and stock go in separately.
          </DialogDescription>
        </DialogHeader>
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
              onChange={(e) =>
                setForm({ ...form, description: e.target.value })
              }
            />
          </Field>
          <Field>
            <FieldLabel htmlFor="product-status">Status</FieldLabel>
            <Select
              value={form.status || undefined}
              onValueChange={(v) =>
                setForm({ ...form, status: v as ProductStatus })
              }
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
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Creating…" : "Create product"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function Products({
  status,
  after,
  onStatusChange,
  onAfterChange,
}: {
  status: ProductStatus | "all"
  after: string | undefined
  onStatusChange: (status: ProductStatus | "all") => void
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["products", status], "/admin/products", product, {
    after,
    onAfterChange,
    query: { status: status === "all" ? undefined : status },
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Products"
        subtitle="This surface sees every status. The storefront sees published only."
      >
        <Select
          value={status}
          onValueChange={(v) => onStatusChange(v as ProductStatus | "all")}
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder="Any status" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">Any status</SelectItem>
            {productStatus.options.map((s) => (
              <SelectItem key={s} value={s}>
                {s}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <NewProductDialog />
      </PageHeading>

      <DataTable
        paged={paged}
        columns={columns}
        empty={{
          title: "No products",
          description:
            status === "all"
              ? "Nothing in the catalogue yet."
              : `Nothing with status ${status}.`,
        }}
      />
    </div>
  )
}
