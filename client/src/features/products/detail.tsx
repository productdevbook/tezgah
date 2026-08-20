import { useState, type FormEvent } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { z } from "zod"

import { get, post } from "@/api/client"
import {
  createDigitalContent,
  digitalContent,
  product,
  type CreateDigitalContent,
  type DigitalContent,
} from "@/api/schemas"
import { DeleteAction } from "@/components/delete-action"
import { DetailField, FieldGrid, Metadata, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { FormError } from "@/components/form-error"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardContent } from "@/components/ui/card"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime, useDetail } from "@/lib/detail"

export function ProductDetail({
  id,
  variantId,
  onVariantIdChange,
}: {
  id: string
  variantId: string | undefined
  onVariantIdChange: (id: string | undefined) => void
}) {
  const result = useDetail(["products"], "/admin/products/{id}", product, id)

  return (
    <div className="max-w-3xl space-y-4">
      <QueryState
        query={result}
        empty={{ title: "No product", description: "Nothing to show." }}
      >
        {(item) => (
          <>
            <DetailHeader back="products" title={item.title} subtitle={item.handle}>
              <Badge variant={item.status === "published" ? "default" : "outline"}>
                {item.status}
              </Badge>
              <Button
                variant="outline"
                size="sm"
                nativeButton={false}
                render={<Link to="/products/$id/edit" params={{ id: item.id }} />}
              >
                Edit
              </Button>
              <DeleteAction
                path="/admin/products/{id}"
                params={{ id: item.id }}
                invalidateKey={["products"]}
                kind="product"
                name={item.title}
              />
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="Handle">{item.handle}</DetailField>
                  <DetailField label="Status">{item.status}</DetailField>
                  <DetailField label="Rejected reason">
                    {item.rejected_reason ?? <Empty />}
                  </DetailField>
                  <DetailField label="Subtitle">{item.subtitle ?? <Empty />}</DetailField>
                  <DetailField label="Discountable">
                    {item.is_discountable ? "yes" : "no"}
                  </DetailField>
                  <DetailField label="Product type">
                    {item.product_type_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Collection">
                    {item.product_collection_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Thumbnail">
                    {item.thumbnail_url ?? <Empty />}
                  </DetailField>
                  <DetailField label="External ID">
                    {item.external_id ?? <Empty />}
                  </DetailField>
                  <DetailField label="Weight">{item.weight ?? <Empty />}</DetailField>
                  <DetailField label="Length">{item.length ?? <Empty />}</DetailField>
                  <DetailField label="Height">{item.height ?? <Empty />}</DetailField>
                  <DetailField label="Width">{item.width ?? <Empty />}</DetailField>
                  <DetailField label="Material">{item.material ?? <Empty />}</DetailField>
                  <DetailField label="HS code">{item.hs_code ?? <Empty />}</DetailField>
                  <DetailField label="Origin country">
                    {item.origin_country ?? <Empty />}
                  </DetailField>
                  <DetailField label="Created">{dateTime(item.created_at)}</DetailField>
                  <DetailField label="Updated">{dateTime(item.updated_at)}</DetailField>
                  <DetailField label="Description" full>
                    {item.description ?? <Empty />}
                  </DetailField>
                  <DetailField label="Metadata" full>
                    <Metadata value={item.metadata} />
                  </DetailField>
                </FieldGrid>
              </CardContent>
            </Card>

            <Card>
              <CardContent className="space-y-3">
                <div className="space-y-1">
                  <h2 className="text-sm font-medium">Digital content</h2>
                  <p className="text-muted-foreground text-xs">
                    A file belongs to one variant, and no route lists a product&rsquo;s
                    variants — paste the variant&rsquo;s id to see or add what it carries.
                  </p>
                </div>
                <DigitalContentByVariant
                  variantId={variantId}
                  onVariantIdChange={onVariantIdChange}
                />
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
  )
}

function DigitalContentByVariant({
  variantId,
  onVariantIdChange,
}: {
  variantId: string | undefined
  onVariantIdChange: (id: string | undefined) => void
}) {
  const [input, setInput] = useState(variantId ?? "")

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    onVariantIdChange(trimmed === "" ? undefined : trimmed)
  }

  return (
    <div className="space-y-4">
      <form className="flex gap-2" onSubmit={submit}>
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="variant id"
          className="font-mono text-xs"
          aria-label="Variant id"
        />
        <Button type="submit" variant="outline">
          Look up
        </Button>
      </form>
      {variantId ? (
        <DigitalContentList variantId={variantId} />
      ) : (
        <p className="text-muted-foreground text-sm">Nothing looked up yet.</p>
      )}
    </div>
  )
}

function DigitalContentList({ variantId }: { variantId: string }) {
  const client = useQueryClient()
  const query = useQuery({
    queryKey: ["digital-content", variantId],
    queryFn: ({ signal }) =>
      get("/admin/variants/{id}/digital-content", {
        signal,
        schema: z.array(digitalContent),
        params: { id: variantId },
      }),
  })

  function refresh() {
    void client.invalidateQueries({ queryKey: ["digital-content", variantId] })
  }

  return (
    <div className="space-y-4">
      <QueryState
        query={query}
        empty={{
          title: "No digital content",
          description: "This variant carries no files yet.",
        }}
      >
        {(items: DigitalContent[]) => (
          <div className="rounded-md border">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>Key</TableHead>
                  <TableHead>Downloads</TableHead>
                  <TableHead>Valid</TableHead>
                  <TableHead>Auto-grant</TableHead>
                  <TableHead className="text-right">Added</TableHead>
                  <TableHead />
                </TableRow>
              </TableHeader>
              <TableBody>
                {items.map((content) => (
                  <TableRow key={content.id}>
                    <TableCell>{content.name}</TableCell>
                    <TableCell className="font-mono text-xs">
                      {content.content_key}
                    </TableCell>
                    <TableCell>{content.max_downloads ?? <Empty />}</TableCell>
                    <TableCell>
                      {content.valid_days ? `${content.valid_days}d` : <Empty />}
                    </TableCell>
                    <TableCell>
                      {content.auto_grant ? <Badge variant="outline">auto</Badge> : null}
                    </TableCell>
                    <TableCell className="text-muted-foreground text-right text-xs">
                      {dateTime(content.created_at)}
                    </TableCell>
                    <TableCell>
                      <DeleteAction
                        path="/admin/digital-content/{id}"
                        params={{ id: content.id }}
                        invalidateKey={["digital-content", variantId]}
                        kind="digital content"
                        name={content.name}
                        onDeleted={refresh}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </QueryState>
      <NewDigitalContent variantId={variantId} onCreated={refresh} />
    </div>
  )
}

const EMPTY_CONTENT_FORM = {
  name: "",
  content_key: "",
  max_downloads: "",
  valid_days: "",
  rank: "",
  auto_grant: false,
}

function NewDigitalContent({
  variantId,
  onCreated,
}: {
  variantId: string
  onCreated: () => void
}) {
  const [form, setForm] = useState(EMPTY_CONTENT_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateDigitalContent) =>
      post("/admin/variants/{id}/digital-content", {
        schema: digitalContent,
        params: { id: variantId },
        body,
      }),
    onSuccess: () => {
      onCreated()
      setForm(EMPTY_CONTENT_FORM)
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const asInt = (v: string) => (v.trim() === "" ? undefined : Number(v))
    const parsed = createDigitalContent.safeParse({
      name: form.name,
      content_key: form.content_key,
      max_downloads: asInt(form.max_downloads),
      valid_days: asInt(form.valid_days),
      rank: asInt(form.rank),
      auto_grant: form.auto_grant,
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
    <form className="space-y-3 border-t pt-4" onSubmit={submit}>
      <h3 className="text-sm font-medium">Add a file</h3>
      {mutation.isError ? <FormError error={mutation.error} /> : null}
      <div className="grid gap-3 sm:grid-cols-2">
        <Field data-invalid={!!fieldErrors.name}>
          <FieldLabel htmlFor="content-name">Name</FieldLabel>
          <Input
            id="content-name"
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
            aria-invalid={!!fieldErrors.name}
          />
          <FieldError>{fieldErrors.name}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.content_key}>
          <FieldLabel htmlFor="content-key">Storage key</FieldLabel>
          <Input
            id="content-key"
            value={form.content_key}
            onChange={(e) => setForm({ ...form, content_key: e.target.value })}
            className="font-mono text-xs"
            aria-invalid={!!fieldErrors.content_key}
          />
          <FieldError>{fieldErrors.content_key}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.max_downloads}>
          <FieldLabel htmlFor="content-max-downloads">Max downloads</FieldLabel>
          <Input
            id="content-max-downloads"
            inputMode="numeric"
            value={form.max_downloads}
            onChange={(e) => setForm({ ...form, max_downloads: e.target.value })}
            placeholder="unlimited"
            aria-invalid={!!fieldErrors.max_downloads}
          />
          <FieldError>{fieldErrors.max_downloads}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.valid_days}>
          <FieldLabel htmlFor="content-valid-days">Valid days</FieldLabel>
          <Input
            id="content-valid-days"
            inputMode="numeric"
            value={form.valid_days}
            onChange={(e) => setForm({ ...form, valid_days: e.target.value })}
            placeholder="never expires"
            aria-invalid={!!fieldErrors.valid_days}
          />
          <FieldError>{fieldErrors.valid_days}</FieldError>
        </Field>
        <Field data-invalid={!!fieldErrors.rank}>
          <FieldLabel htmlFor="content-rank">Rank</FieldLabel>
          <Input
            id="content-rank"
            inputMode="numeric"
            value={form.rank}
            onChange={(e) => setForm({ ...form, rank: e.target.value })}
            aria-invalid={!!fieldErrors.rank}
          />
          <FieldError>{fieldErrors.rank}</FieldError>
        </Field>
        <Field orientation="horizontal">
          <Switch
            id="content-auto-grant"
            checked={form.auto_grant}
            onCheckedChange={(checked) => setForm({ ...form, auto_grant: checked })}
          />
          <FieldLabel htmlFor="content-auto-grant">Grant automatically on purchase</FieldLabel>
        </Field>
      </div>
      <Button type="submit" disabled={mutation.isPending}>
        {mutation.isPending ? "Adding…" : "Add file"}
      </Button>
    </form>
  )
}
