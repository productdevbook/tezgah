import { useMutation } from "@tanstack/react-query"
import { useState } from "react"
import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"

import {
  product,
  productStatus,
  type Product,
  type ProductStatus,
} from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { SearchBox } from "@/components/search-box"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { deleteProducts } from "@/features/batch/api"
import { useT } from "@/panel/i18n"

/** Four of the five mean "not for sale", for four different reasons. */
const HIDDEN: ProductStatus[] = ["draft", "proposed", "rejected", "archived"]

const columns: Columns<Product> = [
  {
    header: "field.title",
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
    header: "field.handle",
    accessorKey: "handle",
    meta: { className: "text-muted-foreground font-mono text-xs" },
  },
  {
    header: "field.status",
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
    header: "field.discountable",
    accessorKey: "is_discountable",
    cell: ({ row }) => (row.original.is_discountable ? "yes" : "no"),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Products({
  status,
  after,
  q,
  by,
  onStatusChange,
  onAfterChange,
  onQChange,
  onByChange,
}: {
  status: ProductStatus | "all"
  after: string | undefined
  q: string | undefined
  by: "created" | "title"
  onStatusChange: (status: ProductStatus | "all") => void
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onByChange: (by: "created" | "title") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["products", status, q ?? "", by],
    "/admin/products",
    product,
    {
      after,
      onAfterChange,
      // The one list the API counts, which is what puts "25 of 41,309" under
      // the table. Every other list leaves `total` null and the pager says
      // nothing rather than counting the rows on screen.
      query: {
        status: status === "all" ? undefined : status,
        q,
        by,
        count: "true",
      },
    }
  )

  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.products.title")}
        subtitle={t("screen.products.subtitle")}
      >
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder="Search title, handle, subtitle"
        />
        <Select
          value={by}
          onValueChange={(value) => onByChange(value as "created" | "title")}
        >
          <SelectTrigger className="w-36" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {/* Two, because the API offers two. A third option here would be
                a claim about pages that are not on screen. */}
            <SelectItem value="created">Newest first</SelectItem>
            <SelectItem value="title">By title</SelectItem>
          </SelectContent>
        </Select>
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
        <Button
          size="sm"
          nativeButton={false}
          render={<Link to="/products/new" />}
        >
          <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          New product
        </Button>
      </PageHeading>

      <DataTable
        paged={paged}
        columns={columns}
        select={{
          id: (row) => row.id,
          actions: (chosen, clear) => (
            <BulkDelete
              chosen={chosen}
              onDone={() => {
                clear()
                void paged.result.refetch()
              }}
            />
          ),
        }}
        rowLink={(row) => (
          <Link
            to="/products/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open ${row.title}`}
          />
        )}
        empty={{
          title: t("screen.products.empty"),
          description:
            status === "all"
              ? t("screen.products.emptyAny")
              : t("screen.products.emptyStatus", { status }),
        }}
      />
    </div>
  )
}

/**
 * What a selection is for. `POST /admin/products/batch` takes rows to write
 * and ids to delete; this is that call with no rows.
 *
 * Asked before it happens and counted in the question, because a bulk delete
 * is the one action on this screen that cannot be undone by doing it again.
 */
function BulkDelete({
  chosen,
  onDone,
}: {
  chosen: string[]
  onDone: () => void
}) {
  const [open, setOpen] = useState(false)

  const mutation = useMutation({
    mutationFn: () => deleteProducts(chosen),
    onSuccess: () => {
      setOpen(false)
      onDone()
    },
  })

  return (
    <>
      <span className="text-sm text-muted-foreground">
        {chosen.length} chosen
      </span>
      <Button
        size="sm"
        variant="destructive"
        onClick={() => setOpen(true)}
        disabled={mutation.isPending}
      >
        Delete
      </Button>
      <AlertDialog open={open} onOpenChange={setOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              Delete {chosen.length} product{chosen.length === 1 ? "" : "s"}?
            </AlertDialogTitle>
            <AlertDialogDescription>
              This cannot be undone. A product an order already names is kept —
              the crate refuses that one and says so, and the rest still go.
            </AlertDialogDescription>
          </AlertDialogHeader>
          {mutation.isError ? (
            <p className="text-sm text-destructive">
              {mutation.error instanceof Error
                ? mutation.error.message
                : "Refused."}
            </p>
          ) : null}
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => mutation.mutate()}
              disabled={mutation.isPending}
            >
              {mutation.isPending ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}
