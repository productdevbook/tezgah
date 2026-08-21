import { useState, type FormEvent } from "react"
import { useQuery } from "@tanstack/react-query"

import { get } from "@/api/client"
import {
  payout,
  payoutBalance,
  payoutLine,
  type Payout,
  type PayoutLine,
} from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { QueryState } from "@/components/query-state"
import { Button } from "@/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { usePagedList } from "@/lib/paged"
import { useT } from "@/panel/i18n"

const columns: Columns<Payout> = [
  { header: "field.reference", accessorKey: "reference" },
  {
    header: "field.referenceId",
    accessorKey: "reference_id",
    meta: { className: "font-mono text-xs text-muted-foreground" },
  },
  {
    header: "field.amount",
    accessorKey: "amount",
    cell: ({ row }) =>
      `${row.original.amount} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "text-right font-mono text-xs" },
  },
]

/**
 * The payouts tab: the list, plus the two lookups that have no list of
 * their own — a balance is one currency's own number, and payout lines are
 * asked for by the order that earned them, not browsed. No row here goes
 * anywhere: `payout` (7 operations) has no `GET .../{id}`, so a payout's own
 * detail is not a screen this panel can draw yet.
 */
export function Payouts({
  after,
  onAfterChange,
  currency,
  onCurrencyChange,
  orderId,
  onOrderIdChange,
  linesAfter,
  onLinesAfterChange,
}: {
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
  currency: string | undefined
  onCurrencyChange: (currency: string | undefined) => void
  orderId: string | undefined
  onOrderIdChange: (id: string | undefined) => void
  linesAfter: string | undefined
  onLinesAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(["payouts"], "/admin/payouts", payout, {
    after,
    onAfterChange,
  })

  return (
    <div className="space-y-6">
      <div className="grid gap-4 sm:grid-cols-2">
        <BalanceLookup
          currency={currency}
          onCurrencyChange={onCurrencyChange}
        />
        <OrderLinesLookup
          orderId={orderId}
          onOrderIdChange={onOrderIdChange}
          after={linesAfter}
          onAfterChange={onLinesAfterChange}
        />
      </div>
      <div className="space-y-3">
        <h2 className="text-sm font-medium">{t("nav.payouts")}</h2>
        <DataTable
          paged={paged}
          columns={columns}
          empty={{
            title: t("empty.payouts"),
            description: t("empty.payoutsWhy"),
          }}
        />
      </div>
    </div>
  )
}

function BalanceLookup({
  currency,
  onCurrencyChange,
}: {
  currency: string | undefined
  onCurrencyChange: (currency: string | undefined) => void
}) {
  const t = useT()
  const [code, setCode] = useState(currency ?? "")

  const query = useQuery({
    queryKey: ["payout-balance", currency],
    queryFn: ({ signal }) =>
      get("/admin/payout-balance/{currency_code}", {
        signal,
        schema: payoutBalance,
        params: { currency_code: currency ?? "" },
      }),
    enabled: currency !== undefined,
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = code.trim().toLowerCase()
    onCurrencyChange(trimmed === "" ? undefined : trimmed)
  }

  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle>{t("field.balance")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <form className="flex gap-2" onSubmit={submit}>
          <Input
            value={code}
            onChange={(e) => setCode(e.target.value)}
            placeholder="usd"
            maxLength={3}
            className="font-mono uppercase"
            aria-label={t("field.currencyCode")}
          />
          <Button type="submit" size="sm" variant="outline">
            Check
          </Button>
        </form>
        {currency ? (
          <QueryState
            query={query}
            empty={{
              title: t("empty.balance"),
              description: t("empty.balanceWhy"),
            }}
          >
            {(balance) => (
              <p className="font-mono text-2xl font-semibold">
                {balance.amount}{" "}
                <span className="text-sm text-muted-foreground uppercase">
                  {balance.currency_code}
                </span>
              </p>
            )}
          </QueryState>
        ) : (
          <p className="text-sm text-muted-foreground">
            What this scope is owed right now, in one currency — negative when a
            refund outran what was already paid out.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

function OrderLinesLookup({
  orderId,
  onOrderIdChange,
  after,
  onAfterChange,
}: {
  orderId: string | undefined
  onOrderIdChange: (id: string | undefined) => void
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const [input, setInput] = useState(orderId ?? "")

  function submit(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    onOrderIdChange(trimmed === "" ? undefined : trimmed)
  }

  return (
    <Card size="sm">
      <CardHeader>
        <CardTitle>{t("frame.payoutLines")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <form className="flex gap-2" onSubmit={submit}>
          <Input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder={t("placeholder.orderId")}
            className="font-mono text-xs"
            aria-label={t("field.orderId")}
          />
          <Button type="submit" size="sm" variant="outline">
            Look up
          </Button>
        </form>
        {orderId ? (
          <OrderLines
            orderId={orderId}
            after={after}
            onAfterChange={onAfterChange}
          />
        ) : (
          <p className="text-sm text-muted-foreground">
            What an order earned the seller and what the marketplace kept, line
            by line.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

const lineColumns: Columns<PayoutLine> = [
  { header: "field.reference", accessorKey: "reference" },
  {
    header: "field.payout",
    accessorKey: "payout_id",
    cell: ({ row }) =>
      row.original.payout_id ?? (
        <span className="text-muted-foreground">unpaid</span>
      ),
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.amount",
    accessorKey: "amount",
    cell: ({ row }) =>
      `${row.original.amount} ${row.original.currency_code.toUpperCase()}`,
    meta: { className: "text-right font-mono text-xs" },
  },
]

function OrderLines({
  orderId,
  after,
  onAfterChange,
}: {
  orderId: string
  after: string | undefined
  onAfterChange: (after: string | undefined) => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["payout-lines", orderId],
    "/admin/orders/{id}/payout-lines",
    payoutLine,
    { after, onAfterChange, params: { id: orderId } }
  )

  return (
    <DataTable
      paged={paged}
      columns={lineColumns}
      empty={{
        title: t("empty.payoutLines"),
        description: t("empty.payoutLinesWhy"),
      }}
    />
  )
}
