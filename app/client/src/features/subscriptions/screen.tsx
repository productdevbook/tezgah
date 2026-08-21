import { Link } from "@tanstack/react-router"

import { subscription, type Subscription } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/components/page-heading"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  SUBSCRIPTION_STATUS,
  type SubscriptionStatus,
} from "@/features/subscriptions/status"
import { useT } from "@/panel/i18n"

const columns: Columns<Subscription> = [
  {
    header: "field.status",
    accessorKey: "status",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant={row.original.ended_at ? "outline" : "default"}>
          {row.original.status}
        </Badge>
        {row.original.cancel_at_period_end ? (
          <Badge variant="outline">ends this period</Badge>
        ) : null}
      </div>
    ),
  },
  {
    header: "field.cycle",
    accessorKey: "cycle",
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.nextCharge",
    accessorKey: "next_billing_at",
    cell: ({ row }) =>
      row.original.ended_at ? (
        <span className="text-muted-foreground">ended</span>
      ) : (
        new Date(row.original.next_billing_at).toLocaleDateString()
      ),
  },
  {
    header: "field.currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "field.dunning",
    accessorKey: "dunning_attempts",
    /* Above zero means a charge failed and is being retried, which is a
       different thing from a cancelled contract. */
    cell: ({ row }) =>
      row.original.dunning_attempts > 0 ? (
        <Badge variant="destructive">{row.original.dunning_attempts}</Badge>
      ) : (
        <span className="text-xs text-muted-foreground">—</span>
      ),
    meta: { className: "text-right" },
  },
]

export function Subscriptions({
  after,
  status,
  ending,
  onAfterChange,
  onStatusChange,
  onEndingChange,
}: {
  after: string | undefined
  status: SubscriptionStatus | "all"
  ending: "all" | "ending" | "staying"
  onAfterChange: (after: string | undefined) => void
  onStatusChange: (status: SubscriptionStatus | "all") => void
  onEndingChange: (ending: "all" | "ending" | "staying") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["subscriptions", status, ending],
    "/admin/subscriptions",
    subscription,
    {
      after,
      onAfterChange,
      query: {
        status: status === "all" ? undefined : status,
        ending:
          ending === "all" ? undefined : ending === "ending" ? "true" : "false",
        count: "true",
      },
    }
  )
  return (
    <div className="space-y-4">
      <PageHeading
        title={t("screen.subscriptions.title")}
        subtitle={t("screen.subscriptions.subtitle")}
      >
        <Select
          value={status}
          onValueChange={(value) =>
            onStatusChange(value as SubscriptionStatus | "all")
          }
        >
          <SelectTrigger className="w-40" size="sm">
            <SelectValue placeholder={t("filter.anyStatus")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("filter.anyStatus")}</SelectItem>
            {SUBSCRIPTION_STATUS.map((one) => (
              <SelectItem key={one} value={one}>
                {one}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={ending}
          onValueChange={(value) =>
            onEndingChange(value as "all" | "ending" | "staying")
          }
        >
          <SelectTrigger className="w-44" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">{t("filter.anyRenewal")}</SelectItem>
            <SelectItem value="ending">{t("filter.ending")}</SelectItem>
            <SelectItem value="staying">{t("filter.renewing")}</SelectItem>
          </SelectContent>
        </Select>
      </PageHeading>
      <DataTable
        paged={paged}
        columns={columns}
        rowLink={(row) => (
          <Link
            to="/subscriptions/$id"
            params={{ id: row.id }}
            className="absolute inset-0"
            aria-label={`Open subscription ${row.id}`}
          />
        )}
        empty={{
          title: t("screen.subscriptions.empty"),
          description: t("screen.subscriptions.emptyAny"),
        }}
      />
    </div>
  )
}
