import {
  tableFeatures,
  useTable,
  type ColumnDef,
  type RowData,
} from "@tanstack/react-table"

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

export type Columns<T extends RowData> = ColumnDef<typeof features, T, unknown>[]

export function DataTable<T extends RowData>({
  paged,
  columns,
  empty,
}: {
  paged: PagedList<T>
  columns: Columns<T>
  empty: { title: string; description: string }
}) {
  return (
    <QueryState query={paged.result} empty={empty}>
      {(page) =>
        page.items.length === 0 ? (
          <Nothing empty={empty} />
        ) : (
          <>
            <div className="rounded-md border">
              <Rows items={page.items} columns={columns} />
            </div>
            {paged.hasPrevious || page.next ? (
              <div className="flex items-center justify-end gap-2 pt-3">
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
        )
      }
    </QueryState>
  )
}

function Rows<T extends RowData>({
  items,
  columns,
}: {
  items: T[]
  columns: Columns<T>
}) {
  const table = useTable({ features, columns, data: items })

  return (
    <Table>
      <TableHeader>
        {table.getHeaderGroups().map((group) => (
          <TableRow key={group.id}>
            {group.headers.map((header) => (
              <TableHead key={header.id} className={styling(header.column.columnDef)}>
                {header.isPlaceholder ? null : <table.FlexRender header={header} />}
              </TableHead>
            ))}
          </TableRow>
        ))}
      </TableHeader>
      <TableBody>
        {table.getRowModel().rows.map((row) => (
          <TableRow key={row.id}>
            {row.getAllCells().map((cell) => (
              <TableCell key={cell.id} className={styling(cell.column.columnDef)}>
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
    <div className="rounded-md border px-4 py-10 text-center">
      <p className="text-sm font-medium">{empty.title}</p>
      <p className="text-muted-foreground mt-1 text-sm">{empty.description}</p>
    </div>
  )
}
