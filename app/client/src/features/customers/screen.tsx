import { Link } from "@tanstack/react-router"

import { customer, type Customer } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"

function name(row: Customer): string | null {
  const parts = [row.first_name, row.last_name].filter(Boolean)
  return parts.length ? parts.join(" ") : (row.company_name ?? null)
}

const columns: Columns<Customer> = [
  {
    header: "Name",
    accessorKey: "first_name",
    cell: ({ row }) =>
      name(row.original) ?? <span className="text-muted-foreground">unnamed</span>,
    meta: { className: "font-medium" },
  },
  {
    header: "Email",
    accessorKey: "email",
    cell: ({ row }) =>
      row.original.email ?? <span className="text-muted-foreground">none</span>,
    meta: { className: "max-w-72 truncate" },
  },
  {
    header: "Account",
    accessorKey: "has_account",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant={row.original.has_account ? "default" : "outline"}>
          {row.original.has_account ? "registered" : "guest"}
        </Badge>
        {/* Erased on request; the orders stay, the person does not. */}
        {row.original.anonymised ? <Badge variant="outline">erased</Badge> : null}
      </div>
    ),
  },
  {
    header: "Since",
    accessorKey: "created_at",
    cell: ({ row }) => new Date(row.original.created_at).toLocaleDateString(),
    meta: { className: "text-right text-muted-foreground text-xs" },
  },
]

export function Customers({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["customers"], "/admin/customers", customer, {
    after,
    onAfterChange,
  })
  return (
    <div className="space-y-4">
      <PageHeading
        title="Customers"
        subtitle="Guests are customers too — a cart makes one before an account does."
      />
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
        empty={{ title: "No customers", description: "Nobody has shopped yet." }}
      />
    </div>
  )
}
