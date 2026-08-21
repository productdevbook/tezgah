import { refundReason, type RefundReason } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"
import { useT } from "@/panel/i18n"

const columns: Columns<RefundReason> = [
  {
    header: "field.code",
    accessorKey: "code",
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.label",
    accessorKey: "label",
    meta: { className: "font-medium" },
  },
  {
    header: "field.description",
    accessorKey: "description",
    cell: ({ row }) => row.original.description ?? <Empty />,
    meta: { className: "max-w-96 truncate" },
  },
]

/** No `GET .../{id}` — only `POST` adds one — so a row here goes nowhere. */
export function RefundReasons({
  after,
  onAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["refund-reasons"],
    "/admin/refund-reasons",
    refundReason,
    {
      after,
      onAfterChange,
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.refundReasons"),
        description: t("frame.refundReasonsWhy"),
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: t("empty.refundReasons"),
        description: t("empty.refundReasonsWhy"),
      }}
    />
  )
}
