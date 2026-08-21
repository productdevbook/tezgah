import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { payment, type Payment } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

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
  state,
  onAfterChange,
  onStateChange,
}: {
  after: string | undefined
  state: "all" | "authorized" | "captured" | "canceled"
  onAfterChange: (after: string | undefined) => void
  onStateChange: (state: "all" | "authorized" | "captured" | "canceled") => void
}) {
  const t = useT()
  const paged = usePagedList(["payments", state], "/admin/payments", payment, {
    after,
    onAfterChange,
    query: {
      state: state === "all" ? undefined : state,
      count: "true",
    },
  })

  return (
    <DataTable
      header={{
        title: t("frame.payments"),
        description: t("frame.paymentsWhy"),
        actions: (
          <Select
            value={state}
            onValueChange={(value) =>
              onStateChange(
                value as "all" | "authorized" | "captured" | "canceled"
              )
            }
          >
            <SelectTrigger className="w-40" size="sm">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">{t("filter.anyPayment")}</SelectItem>
              {/* Three, because a payment row carries two timestamps and the
                  table's own constraint says both cannot be set. */}
              <SelectItem value="authorized">
                {t("filter.authorized")}
              </SelectItem>
              <SelectItem value="captured">{t("filter.captured")}</SelectItem>
              <SelectItem value="canceled">{t("filter.canceled")}</SelectItem>
            </SelectContent>
          </Select>
        ),
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
