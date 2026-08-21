import { useState } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { z } from "zod"

import { get, post } from "@/api/client"
import {
  entitlement,
  order,
  revokeEntitlements,
  type Entitlement,
  type RevokeEntitlements,
} from "@/api/schemas"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog"
import { Empty, Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { FormError } from "@/components/form-error"
import { QueryState } from "@/components/query-state"
import {
  Section,
  SectionBody,
  SectionRow,
  SectionRows,
} from "@/components/section"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime, useDetail } from "@/lib/detail"
import { useT } from "@/panel/i18n"

const SETTLED = [
  "captured",
  "paid",
  "refunded",
  "fulfilled",
  "shipped",
  "delivered",
]
const STUCK = ["canceled", "cancelled", "requires_more", "failed", "declined"]

function tone(status: string): "default" | "outline" | "destructive" {
  if (STUCK.includes(status)) return "destructive"
  if (SETTLED.includes(status)) return "default"
  return "outline"
}

export function OrderDetail({ id }: { id: string }) {
  const t = useT()
  const result = useDetail(["orders"], "/admin/orders/{id}", order, id)

  return (
    <DetailPage
      query={result}
      empty={{
        title: t("empty.order"),
        description: t("general.nothingToShow"),
      }}
      back="orders"
      title={(item) =>
        item.display_id ? `Order #${item.display_id}` : "Order"
      }
      subtitle={(item) => item.email ?? undefined}
      actions={(item) => (
        <>
          <Badge variant={tone(item.status)}>{item.status}</Badge>
          {item.is_draft ? <Badge variant="outline">draft</Badge> : null}
        </>
      )}
      main={(item) => (
        <>
          <Section
            title={t("detail.order.whereItStands")}
            description={t("detail.order.whereItStandsWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.order")}
                value={<Badge variant={tone(item.status)}>{item.status}</Badge>}
              />
              <SectionRow
                label={t("field.payment")}
                value={
                  <Badge variant={tone(item.payment_status)}>
                    {item.payment_status}
                  </Badge>
                }
              />
              <SectionRow
                label={t("field.fulfilment")}
                value={
                  <Badge variant={tone(item.fulfillment_status)}>
                    {item.fulfillment_status}
                  </Badge>
                }
              />
            </SectionRows>
          </Section>

          <Section
            title={t("detail.order.entitlements")}
            description={t("detail.order.entitlementsWhy")}
          >
            <SectionBody>
              <Entitlements
                orderId={id}
                orderLabel={
                  item.display_id ? `order #${item.display_id}` : "this order"
                }
              />
            </SectionBody>
          </Section>
        </>
      )}
      side={(item) => (
        <>
          <Section title={t("detail.order.whoFor")}>
            <SectionRows>
              <SectionRow label={t("field.email")} value={item.email} />
              <SectionRow
                label={t("field.currency")}
                value={<Mono>{item.currency_code.toUpperCase()}</Mono>}
              />
            </SectionRows>
          </Section>

          <Section
            title={t("detail.order.attachedTo")}
            description={t("detail.order.basketWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.paymentCollection")}
                value={
                  item.payment_collection_id ? (
                    <Mono>{item.payment_collection_id}</Mono>
                  ) : null
                }
              />
              <SectionRow
                label={t("field.basket")}
                value={item.basket_id ? <Mono>{item.basket_id}</Mono> : null}
              />
            </SectionRows>
          </Section>

          <Section
            title={t("general.details")}
            description={t("detail.order.versionWhy")}
          >
            <SectionRows>
              <SectionRow
                label={t("field.id")}
                value={<Mono>{item.id}</Mono>}
              />
              <SectionRow label={t("field.number")} value={item.display_id} />
              <SectionRow label={t("field.version")} value={item.version} />
              <SectionRow
                label={t("field.draft")}
                value={item.is_draft ? "Yes" : "No"}
              />
              <SectionRow
                label={t("field.created")}
                value={dateTime(item.created_at)}
              />
              <SectionRow
                label={t("field.completed")}
                value={item.completed_at ? dateTime(item.completed_at) : null}
              />
              <SectionRow
                label={t("field.canceled")}
                value={item.canceled_at ? dateTime(item.canceled_at) : null}
              />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}

/**
 * A plain array, not `Page<T>` — `MAX_ENTITLEMENTS_PER_ORDER` bounds it
 * server-side rather than paging it, so this reads once rather than through
 * `usePagedList`.
 */
function Entitlements({
  orderId,
  orderLabel,
}: {
  orderId: string
  orderLabel: string
}) {
  const t = useT()
  const client = useQueryClient()
  const query = useQuery({
    queryKey: ["orders", orderId, "entitlements"],
    queryFn: ({ signal }) =>
      get("/admin/orders/{id}/entitlements", {
        signal,
        schema: z.array(entitlement),
        params: { id: orderId },
      }),
  })

  return (
    <QueryState
      query={query}
      empty={{
        title: t("empty.entitlements"),
        description: t("empty.entitlementsWhy"),
      }}
    >
      {(items: Entitlement[]) => {
        const active = items.filter((row) => row.revoked_at === null)
        return (
          <div className="space-y-3">
            <div className="rounded-md border">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Digital content</TableHead>
                    <TableHead>Downloads</TableHead>
                    <TableHead>Granted</TableHead>
                    <TableHead>Expires</TableHead>
                    <TableHead className="text-right">Status</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {items.map((row) => (
                    <TableRow key={row.id}>
                      <TableCell className="font-mono text-xs">
                        {row.digital_content_id}
                      </TableCell>
                      <TableCell>
                        {row.download_count}
                        {row.max_downloads !== null
                          ? ` / ${row.max_downloads}`
                          : null}
                      </TableCell>
                      <TableCell>{dateTime(row.granted_at)}</TableCell>
                      <TableCell>
                        {row.expires_at ? dateTime(row.expires_at) : <Empty />}
                      </TableCell>
                      <TableCell className="text-right">
                        {row.revoked_at ? (
                          <Badge variant="destructive">revoked</Badge>
                        ) : (
                          <Badge>active</Badge>
                        )}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
            {active.length > 0 ? (
              <RevokeEntitlementsAction
                orderId={orderId}
                orderLabel={orderLabel}
                activeCount={active.length}
                onRevoked={() =>
                  void client.invalidateQueries({
                    queryKey: ["orders", orderId, "entitlements"],
                  })
                }
              />
            ) : null}
          </div>
        )
      }}
    </QueryState>
  )
}

/**
 * `POST .../entitlements/revoke` takes every active entitlement back at
 * once — there is no per-row revoke — so the dialog names the order and how
 * many rights leave with it, the same discipline `delete-action.tsx` names a
 * record rather than asking "Are you sure?".
 */
function RevokeEntitlementsAction({
  orderId,
  orderLabel,
  activeCount,
  onRevoked,
}: {
  orderId: string
  orderLabel: string
  activeCount: number
  onRevoked: () => void
}) {
  const [open, setOpen] = useState(false)
  const [reason, setReason] = useState("")
  const [reasonError, setReasonError] = useState<string>()

  const mutation = useMutation({
    mutationFn: (body: RevokeEntitlements) =>
      post("/admin/orders/{id}/entitlements/revoke", {
        schema: z.array(entitlement),
        params: { id: orderId },
        body,
      }),
    onSuccess: () => {
      onRevoked()
      setOpen(false)
      setReason("")
    },
  })

  function confirm() {
    const parsed = revokeEntitlements.safeParse({
      reason: reason.trim() === "" ? undefined : reason,
    })
    if (!parsed.success) {
      setReasonError(parsed.error.issues[0]?.message)
      return
    }
    setReasonError(undefined)
    mutation.mutate(parsed.data)
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogTrigger render={<Button variant="destructive" size="sm" />}>
        Revoke entitlements
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            Revoke {activeCount} entitlement{activeCount === 1 ? "" : "s"} for{" "}
            {orderLabel}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            Takes back every digital right {orderLabel} currently grants. It
            cannot be undone from here.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <Field data-invalid={!!reasonError}>
          <FieldLabel htmlFor="revoke-reason">Reason (optional)</FieldLabel>
          <Input
            id="revoke-reason"
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            aria-invalid={!!reasonError}
          />
          <FieldError>{reasonError}</FieldError>
        </Field>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            disabled={mutation.isPending}
            onClick={confirm}
          >
            {mutation.isPending ? "Revoking…" : "Revoke"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
