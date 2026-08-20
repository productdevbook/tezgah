import { refundReason, type RefundReason } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"

const columns: Columns<RefundReason> = [
  {
    header: "Code",
    accessorKey: "code",
    meta: { className: "font-mono text-xs" },
  },
  { header: "Label", accessorKey: "label", meta: { className: "font-medium" } },
  {
    header: "Description",
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
        title: "Refund reasons",
        description:
          "The reasons a refund can be given against, kept so a report can count them.",
      }}
      paged={paged}
      columns={columns}
      empty={{
        title: "No refund reasons",
        description:
          "A refund can be given without one, but nothing here explains why yet.",
      }}
    />
  )
}
