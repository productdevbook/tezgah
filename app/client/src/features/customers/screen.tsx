import { Link } from "@tanstack/react-router"

import { customer, type Customer } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import { SearchBox } from "@/components/search-box"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useT } from "@/panel/i18n"

function name(row: Customer): string | null {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? null)
}

const columns: Columns<Customer> = [
  {
    header: "field.name",
    accessorKey: "first_name",
    cell: ({ row }) =>
      name(row.original) ?? (
        <span className="text-muted-foreground">unnamed</span>
      ),
    meta: { className: "font-medium" },
  },
  {
    header: "field.email",
    accessorKey: "email",
    cell: ({ row }) =>
      row.original.email ?? <span className="text-muted-foreground">none</span>,
    meta: { className: "max-w-72 truncate" },
  },
  {
    header: "field.account",
    accessorKey: "has_account",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant={row.original.has_account ? "default" : "outline"}>
          {row.original.has_account ? "registered" : "guest"}
        </Badge>
        {/* Erased on request; the orders stay, the person does not. */}
        {row.original.anonymised ? (
          <Badge variant="outline">erased</Badge>
        ) : null}
      </div>
    ),
  },
  {
    header: "field.since",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleDateString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Customers({
  after,
  q,
  by,
  onAfterChange,
  onQChange,
  onByChange,
}: {
  after: string | undefined
  q: string | undefined
  by: "created" | "email"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onByChange: (by: "created" | "email") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["customers", q ?? ""],
    "/admin/customers",
    customer,
    {
      after,
      onAfterChange,
      query: { q, by, count: "true" },
    }
  )
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.customers.title")}
        subtitle={t("screen.customers.subtitle")}
      >
        <Select
          value={by}
          onValueChange={(value) => onByChange(value as "created" | "email")}
        >
          <SelectTrigger className="w-36" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {/* Two, because the crate orders this list two ways. */}
            <SelectItem value="created">Newest first</SelectItem>
            <SelectItem value="email">By e-mail</SelectItem>
          </SelectContent>
        </Select>
        <SearchBox
          value={q}
          onChange={onQChange}
          placeholder="Search name, e-mail, company"
        />
      </PageHeading>
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/customers/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open ${name(row) ?? "customer"}`}
          />
        )}
        empty={{
          title: t("screen.customers.empty"),
          description: q
            ? t("search.nothingMatches", { q })
            : t("screen.customers.emptyAny"),
        }}
      />
    </div>
  )
}
