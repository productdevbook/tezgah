import {
  tableFeatures,
  useTable,
  type ColumnDef,
  type RowData,
} from "@tanstack/react-table"
import type { ReactNode } from "react"

import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import type { PagedList } from "@/lib/paged"

/**
 * No feature is registered: v9 makes every one of them explicit, and this
 * table sorts nothing and filters nothing on the client. Every list handler
 * takes a cursor and a limit and nothing else, so a header that sorted what is
 * on screen would be claiming something about the pages that are not.
 */
const features = tableFeatures({})

export type Columns<T extends RowData> = ColumnDef<
  typeof features,
  T,
  unknown
>[]

export function DataTable<T extends RowData>({
  paged,
  columns,
  empty,
  header,
  rowLink,
}: {
  paged: PagedList<T>
  columns: Columns<T>
  empty: { title: string; description: string }
  /**
   * What this list is, drawn inside the same frame as the rows.
   *
   * A tab's content has no page title of its own — `/tax/rates` is one of
   * three tables under one heading — so without this a screen is a grid of
   * rows and no sentence saying what they are.
   */
  header?: { title: string; description?: string; actions?: ReactNode }
  /**
   * A row goes to `rowLink(row)`'s address by way of a real anchor stretched
   * over it — `absolute inset-0` inside the row's first cell, the row itself
   * `position: relative` — never an `onClick`, which a middle click or
   * "open in new tab" would silently do nothing with.
   */
  rowLink?: (row: T) => ReactNode
}) {
  const page = paged.result.data

  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      {header ? <Header {...header} /> : null}
      {page === undefined ? (
        // Loading, refusal and drift are padded; a table draws its own.
        <div className="px-6 py-6">
          <QueryState query={paged.result} empty={empty}>
            {() => null}
          </QueryState>
        </div>
      ) : page.items.length === 0 ? (
        <Nothing empty={empty} />
      ) : (
        <>
          <Rows items={page.items} columns={columns} rowLink={rowLink} />
          {paged.hasPrevious || page.next ? (
            <div className="flex items-center justify-end gap-2 border-t px-6 py-3">
              <Button
                variant="outline"
                size="sm"
                disabled={!paged.hasPrevious}
                onClick={paged.back}
              >
                Back
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!page.next}
                onClick={() => page.next && paged.forward(page.next)}
              >
                Next
              </Button>
            </div>
          ) : null}
        </>
      )}
    </div>
  )
}

/**
 * The same frame, for a list that answers a plain array rather than
 * `Page<T>` and so builds its own table — `fulfilment`'s providers, `tax`'s
 * registrations, the store's currencies. Nothing pages, so nothing here does
 * either.
 */
export function TableFrame({
  header,
  children,
}: {
  header?: { title: string; description?: string; actions?: ReactNode }
  children: ReactNode
}) {
  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      {header ? <Header {...header} /> : null}
      {children}
    </div>
  )
}

/**
 * There is no search box, no filter and no sortable header here on purpose:
 * every list handler in the crate takes a cursor and a limit and nothing
 * else, so any of the three would be claiming something about the pages that
 * are not on screen. It is a gap in the API rather than in this file —
 * `docs/architecture.md` carries it.
 */
export function Header({
  title,
  description,
  actions,
}: {
  title: string
  description?: string
  actions?: ReactNode
}) {
  return (
    <header className="flex items-start justify-between gap-4 border-b px-6 py-4">
      <div className="min-w-0">
        <h2 className="truncate text-base font-medium">{title}</h2>
        {description ? (
          <p className="mt-0.5 text-sm text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {actions ? (
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      ) : null}
    </header>
  )
}

function Rows<T extends RowData>({
  items,
  columns,
  rowLink,
}: {
  items: T[]
  columns: Columns<T>
  rowLink?: (row: T) => ReactNode
}) {
  const table = useTable({ features, columns, data: items })

  return (
    <Table>
      <TableHeader>
        {table.getHeaderGroups().map((group) => (
          <TableRow key={group.id}>
            {group.headers.map((header) => (
              <TableHead
                key={header.id}
                className={styling(header.column.columnDef)}
              >
                {header.isPlaceholder ? null : (
                  <table.FlexRender header={header} />
                )}
              </TableHead>
            ))}
          </TableRow>
        ))}
      </TableHeader>
      <TableBody>
        {table.getRowModel().rows.map((row) => (
          <TableRow key={row.id} className={rowLink ? "relative" : undefined}>
            {row.getAllCells().map((cell, index) => (
              <TableCell
                key={cell.id}
                className={styling(cell.column.columnDef)}
              >
                {rowLink && index === 0 ? rowLink(row.original) : null}
                <table.FlexRender cell={cell} />
              </TableCell>
            ))}
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}

function styling(def: { meta?: unknown }): string | undefined {
  return (def.meta as { className?: string } | undefined)?.className
}

function Nothing({ empty }: { empty: { title: string; description: string } }) {
  return (
    <div className="px-4 py-10 text-center">
      <p className="text-sm font-medium">{empty.title}</p>
      <p className="mt-1 text-sm text-muted-foreground">{empty.description}</p>
    </div>
  )
}
