import { Link } from "@tanstack/react-router"

import { payment, type Payment } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<Payment> = [
  {
    header: "Amount",
    accessorKey: "amount",
    cell: ({ row }) =>
      `${row.original.amount.amount} ${row.original.amount.currency.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "Payment collection",
    accessorKey: "payment_collection_id",
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "Captured",
    accessorKey: "captured_at",
    cell: ({ row }) => (row.original.captured_at ? "captured" : <Empty />),
  },
  {
    header: "Canceled",
    accessorKey: "canceled_at",
    cell: ({ row }) => (row.original.canceled_at ? "canceled" : <Empty />),
    meta: { className: "text-right" },
  },
]

export function Payments({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const paged = usePagedList(["payments"], "/admin/payments", payment, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: "Payments",
        description:
          "Authorising and capturing are separate acts, so a payment that exists is not yet money taken.",
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/payments/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open payment ${row.id}`}
        />
      )}
      empty={{
        title: "No payments",
        description: "Nothing has been taken yet.",
      }}
    />
  )
}
