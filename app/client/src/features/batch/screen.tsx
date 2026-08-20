import { useMutation } from "@tanstack/react-query"
import { useState } from "react"

import { TableFrame } from "@/components/data-table"
import { Mono } from "@/components/detail-fields"
import { PageHeading } from "@/components/page-heading"
import { Section, SectionBody } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Textarea } from "@/components/ui/textarea"
import {
  COLUMNS,
  exportProducts,
  importProducts,
  type ExportRow,
  type ImportResult,
} from "@/features/batch/api"
import { fromCsv, toCsv } from "@/features/batch/csv"

/**
 * The way a shop changes four hundred prices.
 *
 * Export a page, edit it in whatever the shop already uses, put it back. The
 * export's columns and the import's are the same, which is what makes that a
 * round trip rather than two unrelated screens.
 *
 * A page at a time, and it says so: `GET /admin/products/export` is a cursor
 * list like any other, so this exports what is on the page rather than
 * pretending to hand over a whole catalogue. A shop with four hundred
 * variants asks four times.
 */
export function Batch() {
  return (
    <div className="space-y-4">
      <PageHeading
        title="Import and export"
        subtitle="A page of variants out as CSV, edited, and back in. Same columns both ways."
      />
      <Export />
      <Import />
    </div>
  )
}

function Export() {
  const [currency, setCurrency] = useState("")
  const [after, setAfter] = useState<string | undefined>(undefined)
  const [rows, setRows] = useState<ExportRow[]>([])
  const [next, setNext] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: () =>
      exportProducts(after, currency.trim() === "" ? undefined : currency),
    onSuccess: (page) => {
      setRows(page.items)
      setNext(page.next)
    },
  })

  const csv = toCsv(
    COLUMNS,
    rows.map((row) => [
      row.handle,
      row.product_title,
      "",
      "",
      row.status,
      row.variant_title,
      row.sku ?? "",
      row.price_amount ?? "",
      row.price_currency ?? "",
    ])
  )

  return (
    <Section
      title="Export"
      description="One page of variants, flat. A price needs a currency to be a price, so an export naming none leaves those two columns empty."
    >
      <SectionBody>
        <div className="flex flex-col gap-4">
          <div className="flex flex-wrap items-end gap-2">
            <Field className="w-40">
              <FieldLabel htmlFor="export-currency">Currency</FieldLabel>
              <Input
                id="export-currency"
                className="uppercase"
                placeholder="TRY"
                value={currency}
                onChange={(event) => setCurrency(event.target.value)}
              />
            </Field>
            <Button
              onClick={() => {
                setAfter(undefined)
                mutation.mutate()
              }}
              disabled={mutation.isPending}
            >
              {mutation.isPending ? "Reading…" : "Export a page"}
            </Button>
            {next ? (
              <Button
                variant="outline"
                onClick={() => {
                  setAfter(next)
                  mutation.mutate()
                }}
                disabled={mutation.isPending}
              >
                Next page
              </Button>
            ) : null}
          </div>

          {mutation.isError ? (
            <p className="text-sm text-destructive">
              {mutation.error instanceof Error
                ? mutation.error.message
                : "Refused."}
            </p>
          ) : null}

          {rows.length > 0 ? (
            <>
              <Textarea
                readOnly
                rows={10}
                className="font-mono text-xs"
                value={csv}
                aria-label="The exported rows, as CSV"
              />
              <p className="text-xs text-muted-foreground">
                {rows.length} rows. Copy this into a spreadsheet — there is no
                file to download, because a page is a page rather than a
                catalogue.
              </p>
            </>
          ) : null}
        </div>
      </SectionBody>
    </Section>
  )
}

function Import() {
  const [text, setText] = useState("")
  const [refusal, setRefusal] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: (rows: unknown[]) => importProducts(rows),
  })

  function submit() {
    setRefusal(null)
    const { header, rows } = fromCsv(text)

    // A file whose columns are in another order is readable; one missing a
    // column the crate needs is not, and saying which is missing beats
    // sending four hundred rows the server will reject one at a time.
    for (const required of ["handle", "title"]) {
      if (!header.includes(required)) {
        setRefusal(`the header has no ${required} column`)
        return
      }
    }

    const index = (name: string) => header.indexOf(name)
    const at = (row: string[], name: string) => {
      const i = index(name)
      const value = i === -1 ? "" : (row[i] ?? "")
      return value.trim() === "" ? undefined : value
    }

    const parsed = rows.map((row) => ({
      handle: at(row, "handle"),
      title: at(row, "title"),
      subtitle: at(row, "subtitle"),
      description: at(row, "description"),
      status: at(row, "status"),
      variant_title: at(row, "variant_title"),
      sku: at(row, "sku"),
      price_amount: at(row, "price_amount"),
      price_currency: at(row, "price_currency"),
    }))

    if (parsed.length === 0) {
      setRefusal("there are no rows under that header")
      return
    }

    mutation.mutate(parsed)
  }

  return (
    <Section
      title="Import"
      description="Every row is a variant. A handle that exists is updated; one that does not is created. Nothing is deleted here — that takes an id, and a spreadsheet is the wrong place to name one."
    >
      <SectionBody>
        <div className="flex flex-col gap-4">
          <Field>
            <FieldLabel htmlFor="import-csv">CSV</FieldLabel>
            <Textarea
              id="import-csv"
              rows={10}
              className="font-mono text-xs"
              placeholder={COLUMNS.join(",")}
              value={text}
              onChange={(event) => setText(event.target.value)}
            />
            <FieldDescription>
              The first line is the header. Columns may be in any order; a
              column that is not there is left alone.
            </FieldDescription>
          </Field>

          {refusal ? (
            <p className="text-sm text-destructive">{refusal}</p>
          ) : null}
          {mutation.isError ? (
            <p className="text-sm text-destructive">
              {mutation.error instanceof Error
                ? mutation.error.message
                : "Refused."}
            </p>
          ) : null}

          <div>
            <Button
              onClick={submit}
              disabled={mutation.isPending || text.trim() === ""}
            >
              {mutation.isPending ? "Writing…" : "Import"}
            </Button>
          </div>

          {mutation.data ? <Outcome result={mutation.data} /> : null}
        </div>
      </SectionBody>
    </Section>
  )
}

/**
 * What came back, rejections and all.
 *
 * A batch that half worked is the ordinary case, not the exception: the crate
 * applies what it can and hands back a row number and a reason for each one
 * it could not. Hiding those behind "some rows failed" is what makes an
 * import unusable.
 */
function Outcome({ result }: { result: ImportResult }) {
  const applied =
    result.applied ??
    (result.created ?? 0) + (result.updated ?? 0) + (result.deleted ?? 0)

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge>{applied} applied</Badge>
        {result.created !== undefined ? (
          <Badge variant="outline">{result.created} created</Badge>
        ) : null}
        {result.updated !== undefined ? (
          <Badge variant="outline">{result.updated} updated</Badge>
        ) : null}
        {result.rejected.length > 0 ? (
          <Badge variant="destructive">{result.rejected.length} rejected</Badge>
        ) : null}
      </div>

      {result.rejected.length > 0 ? (
        <TableFrame
          header={{
            title: "Rejected",
            description:
              "By row number, counting from the first row under the header.",
          }}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="w-20">Row</TableHead>
                <TableHead>Reason</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {result.rejected.map((one) => (
                <TableRow key={`${one.row}-${one.reason}`}>
                  <TableCell>
                    <Mono>{one.row}</Mono>
                  </TableCell>
                  <TableCell>{one.reason}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      ) : null}
    </div>
  )
}
