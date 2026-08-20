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
import { ActionMenu } from "@/components/action-menu"
import { DeleteAction } from "@/components/delete-action"
import { Metadata, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { TwoColumnPage } from "@/components/two-column"
import { FormError } from "@/components/form-error"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
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
import { Variants } from "@/features/products/variants"
import { dateTime, useDetail } from "@/lib/detail"

/**
 * A product's page: a stack of sections rather than one card of twenty
 * fields, each owning one part of the record and carrying what can be done
 * to it. `<Outlet />` in the route above this draws `/products/$id/edit` as a
 * drawer over the top, so editing does not take the page away.
 */
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
    <QueryState
      query={result}
      empty={{ title: "No product", description: "Nothing to show." }}
    >
      {(item) => (
        <div className="flex flex-col gap-4">
          <DetailHeader
            back="products"
            title={item.title}
            subtitle={item.handle}
          >
            <Badge
              variant={item.status === "published" ? "default" : "outline"}
            >
              {item.status}
            </Badge>
            <DeleteAction
              path="/admin/products/{id}"
              params={{ id: item.id }}
              invalidateKey={["products"]}
              kind="product"
              name={item.title}
            />
          </DetailHeader>

          <TwoColumnPage
            main={
              <>
                <Section
                  title="General"
                  actions={
                    <ActionMenu
                      groups={[
                        [
                          {
                            label: "Edit",
                            render: (
                              <Link
                                to="/products/$id/edit"
                                params={{ id: item.id }}
                              />
                            ),
                          },
                        ],
                      ]}
                    />
                  }
                >
                  <SectionRows>
                    <SectionRow label="Title" value={item.title} />
                    <SectionRow label="Subtitle" value={item.subtitle} />
                    <SectionRow
                      label="Handle"
                      value={
                        <span className="font-mono text-xs">{item.handle}</span>
                      }
                    />
                    <SectionRow label="Description" value={item.description} />
                    <SectionRow
                      label="Discountable"
                      value={item.is_discountable ? "Yes" : "No"}
                    />
                    <SectionRow
                      label="Rejected reason"
                      value={item.rejected_reason}
                    />
                  </SectionRows>
                </Section>

                <Variants productId={item.id} />

                <Section
                  title="Digital content"
                  description="A file belongs to one variant — take an id from the variants above to see or add what it carries."
                >
                  <div className="px-6 py-4">
                    <DigitalContentByVariant
                      variantId={variantId}
                      onVariantIdChange={onVariantIdChange}
                    />
                  </div>
                </Section>
              </>
            }
            side={
              <>
                <Section
                  title="Organisation"
                  actions={
                    <ActionMenu
                      groups={[
                        [
                          {
                            label: "Edit",
                            render: (
                              <Link
                                to="/products/$id/organisation"
                                params={{ id: item.id }}
                              />
                            ),
                          },
                        ],
                      ]}
                    />
                  }
                >
                  <SectionRows>
                    <SectionRow
                      label="Product type"
                      value={item.product_type_id}
                    />
                    <SectionRow
                      label="Collection"
                      value={item.product_collection_id}
                    />
                    <SectionRow label="External ID" value={item.external_id} />
                  </SectionRows>
                </Section>

                <Section
                  title="Media"
                  actions={
                    <ActionMenu
                      groups={[
                        [
                          {
                            label: "Edit",
                            render: (
                              <Link
                                to="/products/$id/media"
                                params={{ id: item.id }}
                              />
                            ),
                          },
                        ],
                      ]}
                    />
                  }
                >
                  <SectionRows>
                    <SectionRow label="Thumbnail" value={item.thumbnail_url} />
                  </SectionRows>
                </Section>

                <Section
                  title="Shipping"
                  description="What a carrier needs to quote, and what customs needs to let it through."
                  actions={
                    <ActionMenu
                      groups={[
                        [
                          {
                            label: "Edit",
                            render: (
                              <Link
                                to="/products/$id/attributes"
                                params={{ id: item.id }}
                              />
                            ),
                          },
                        ],
                      ]}
                    />
                  }
                >
                  <SectionRows>
                    <SectionRow label="Weight" value={item.weight} />
                    <SectionRow label="Length" value={item.length} />
                    <SectionRow label="Height" value={item.height} />
                    <SectionRow label="Width" value={item.width} />
                    <SectionRow label="Material" value={item.material} />
                    <SectionRow label="HS code" value={item.hs_code} />
                    <SectionRow
                      label="Origin country"
                      value={item.origin_country}
                    />
                  </SectionRows>
                </Section>

                <Section title="Metadata">
                  <div className="px-6 py-4">
                    <Metadata value={item.metadata} />
                  </div>
                </Section>

                <Section title="Details">
                  <SectionRows>
                    <SectionRow
                      label="ID"
                      value={
                        <span className="font-mono text-xs">{item.id}</span>
                      }
                    />
                    <SectionRow
                      label="Created"
                      value={dateTime(item.created_at)}
                    />
                    <SectionRow
                      label="Updated"
                      value={dateTime(item.updated_at)}
                    />
                  </SectionRows>
                </Section>
              </>
            }
          />
        </div>
      )}
    </QueryState>
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
        <p className="text-sm text-muted-foreground">Nothing looked up yet.</p>
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
                      {content.valid_days ? (
                        `${content.valid_days}d`
                      ) : (
                        <Empty />
                      )}
                    </TableCell>
                    <TableCell>
                      {content.auto_grant ? (
                        <Badge variant="outline">auto</Badge>
                      ) : null}
                    </TableCell>
                    <TableCell className="text-right text-xs text-muted-foreground">
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
            onChange={(e) =>
              setForm({ ...form, max_downloads: e.target.value })
            }
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
            onCheckedChange={(checked) =>
              setForm({ ...form, auto_grant: checked })
            }
          />
          <FieldLabel htmlFor="content-auto-grant">
            Grant automatically on purchase
          </FieldLabel>
        </Field>
      </div>
      <Button type="submit" disabled={mutation.isPending}>
        {mutation.isPending ? "Adding…" : "Add file"}
      </Button>
    </form>
  )
}
