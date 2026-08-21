import {
  tableFeatures,
  useTable,
  type ColumnDef,
  type RowData,
} from "@tanstack/react-table"
import { useState, type ReactNode } from "react"

import { Checkbox } from "@/components/ui/checkbox"
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
import { useT } from "@/panel/i18n"

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
  select,
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
   * Multi-select, when there is something to do with a selection.
   *
   * Off unless a screen passes this, because a checkbox column on a list with
   * no bulk action is a control that does nothing. `id` is what a row is
   * named by — the caller says which field, because not every row here is
   * keyed the same way.
   */
  select?: {
    id: (row: T) => string
    /** Drawn in place of the header's own actions while anything is chosen. */
    actions: (chosen: string[], clear: () => void) => ReactNode
  }
  /**
   * A row goes to `rowLink(row)`'s address by way of a real anchor stretched
   * over it — `absolute inset-0` inside the row's first cell, the row itself
   * `position: relative` — never an `onClick`, which a middle click or
   * "open in new tab" would silently do nothing with.
   */
  rowLink?: (row: T) => ReactNode
}) {
  const t = useT()
  const page = paged.result.data
  const [chosen, setChosen] = useState<string[]>([])

  // A selection is about rows that are on screen. Paging away and back would
  // otherwise leave ids selected that nobody can see, and a bulk action would
  // act on them.
  const visible = page?.items ?? []
  const ids = select ? visible.map(select.id) : []
  const kept = chosen.filter((id) => ids.includes(id))
  if (kept.length !== chosen.length) setChosen(kept)

  return (
    <div className="overflow-hidden rounded-xl border bg-card">
      {header ? (
        <Header
          {...header}
          actions={
            select && chosen.length > 0
              ? select.actions(chosen, () => setChosen([]))
              : header.actions
          }
        />
      ) : null}
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
          <Rows
            items={page.items}
            columns={columns}
            rowLink={rowLink}
            select={
              select
                ? {
                    id: select.id,
                    chosen,
                    toggle: (id) =>
                      setChosen((was: string[]) =>
                        was.includes(id)
                          ? was.filter((one) => one !== id)
                          : [...was, id]
                      ),
                    all: () =>
                      setChosen((was) =>
                        was.length === ids.length ? [] : ids
                      ),
                  }
                : undefined
            }
          />
          {paged.hasPrevious || page.next || page.total !== undefined ? (
            <div className="flex items-center justify-end gap-2 border-t px-6 py-3">
              {/* Only when the API answered one. A cursor page does not know
                  how many rows are behind it, so most lists say nothing here
                  rather than counting what is on screen and calling it a
                  total. */}
              {page.total !== undefined && page.total !== null ? (
                <span className="mr-auto text-sm text-muted-foreground">
                  {t("table.showing", {
                    shown: page.items.length,
                    total: page.total.toLocaleString(),
                  })}
                </span>
              ) : null}
              <Button
                variant="outline"
                size="sm"
                disabled={!paged.hasPrevious}
                onClick={paged.back}
              >
                {t("table.back")}
              </Button>
              <Button
                variant="outline"
                size="sm"
                disabled={!page.next}
                onClick={() => page.next && paged.forward(page.next)}
              >
                {t("table.next")}
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

type Selection<T> = {
  id: (row: T) => string
  chosen: string[]
  toggle: (id: string) => void
  all: () => void
}

function Rows<T extends RowData>({
  items,
  columns,
  rowLink,
  select,
}: {
  items: T[]
  columns: Columns<T>
  rowLink?: (row: T) => ReactNode
  select?: Selection<T>
}) {
  const t = useT()
  const table = useTable({ features, columns, data: items })

  return (
    <Table>
      <TableHeader>
        {table.getHeaderGroups().map((group) => (
          <TableRow key={group.id}>
            {select ? (
              <TableHead className="w-10">
                <Checkbox
                  aria-label={t("table.chooseEvery")}
                  checked={
                    select.chosen.length > 0 &&
                    select.chosen.length === items.length
                  }
                  indeterminate={
                    select.chosen.length > 0 &&
                    select.chosen.length < items.length
                  }
                  onCheckedChange={select.all}
                />
              </TableHead>
            ) : null}
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
            {select ? (
              // Outside the stretched link's cell on purpose: a checkbox
              // inside it would be covered by the anchor, and clicking to
              // choose a row would open it instead.
              <TableCell className="w-10">
                <Checkbox
                  aria-label={t("table.chooseThis")}
                  checked={select.chosen.includes(select.id(row.original))}
                  onCheckedChange={() => select.toggle(select.id(row.original))}
                />
              </TableCell>
            ) : null}
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
