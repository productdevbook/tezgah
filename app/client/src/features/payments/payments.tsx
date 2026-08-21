import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { payment, type Payment } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<Payment> = [
  {
    header: "field.amount",
    accessorKey: "amount",
    cell: ({ row }) =>
      `${row.original.amount.amount} ${row.original.amount.currency.toUpperCase()}`,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.paymentCollection",
    accessorKey: "payment_collection_id",
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "field.captured",
    accessorKey: "captured_at",
    cell: ({ row }) => (row.original.captured_at ? "captured" : <Empty />),
  },
  {
    header: "field.canceled",
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
  const t = useT()
  const paged = usePagedList(["payments"], "/admin/payments", payment, {
    after,
    onAfterChange,
  })

  return (
    <DataTable
      header={{
        title: t("frame.payments"),
        description: t("frame.paymentsWhy"),
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
        title: t("empty.payments"),
        description: t("empty.paymentsWhy"),
      }}
    />
  )
}
