import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useState } from "react"

import { apiMutator } from "@/api/mutator"
import { ActionMenu } from "@/components/action-menu"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import type { Subscription } from "@/api/schemas"

/**
 * What a shop does to a contract after it exists.
 *
 * Offered by status, the same way a product's transitions are: pausing a
 * cancelled contract is not a thing to be refused, it is a thing not to be
 * offered. `expired` gets nothing — a contract that ran out is history.
 */
function movesFor(status: string): { label: string; path: string }[] {
  switch (status) {
    case "active":
      return [
        { label: "Skip the next period", path: "skip" },
        { label: "Pause", path: "pause" },
        { label: "Cancel", path: "cancel" },
      ]
    // No "bill the cycle it owes" here: `POST .../renew` needs a recurring
    // payment provider and this binary has none, so the route is not bound.
    // Offering it would be a menu item that always answers 404.
    case "past_due":
      return [
        { label: "Pause", path: "pause" },
        { label: "Cancel", path: "cancel" },
      ]
    case "paused":
      return [
        { label: "Resume", path: "resume" },
        { label: "Cancel", path: "cancel" },
      ]
    default:
      return []
  }
}

async function act(id: string, path: string, body?: unknown): Promise<void> {
  await apiMutator(`/admin/subscriptions/${encodeURIComponent(id)}/${path}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    // Every one of these takes a body, even the two whose body is empty:
    // `Pause` and `Cancel` have optional fields, and a POST with no body at
    // all is a different request than one with `{}`.
    body: JSON.stringify(body ?? {}),
  })
}

export function SubscriptionActions({ item }: { item: Subscription }) {
  const client = useQueryClient()
  const [cancelling, setCancelling] = useState(false)
  const [immediately, setImmediately] = useState(false)
  const [reason, setReason] = useState("")

  const mutation = useMutation({
    mutationFn: ({ path, body }: { path: string; body?: unknown }) =>
      act(item.id, path, body),
    onSuccess: () => {
      setCancelling(false)
      setReason("")
      setImmediately(false)
      void client.invalidateQueries({ queryKey: ["subscriptions"] })
    },
  })

  const moves = movesFor(item.status)
  if (moves.length === 0) return null

  return (
    <>
      <ActionMenu
        groups={[
          moves.map((one) => ({
            label: one.label,
            destructive: one.path === "cancel",
            onSelect: () =>
              one.path === "cancel"
                ? setCancelling(true)
                : mutation.mutate({ path: one.path }),
          })),
        ]}
      />
      <Dialog open={cancelling} onOpenChange={setCancelling}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Cancel this subscription</DialogTitle>
            <DialogDescription>
              Left as it is, the contract stops at the end of the period the
              customer has already paid for — which is what they are owed.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Field orientation="horizontal">
              <Switch
                id="immediately"
                checked={immediately}
                onCheckedChange={setImmediately}
              />
              <FieldLabel htmlFor="immediately">Stop it now instead</FieldLabel>
            </Field>
            <Field>
              <FieldLabel htmlFor="cancel-reason">Reason</FieldLabel>
              <Input
                id="cancel-reason"
                value={reason}
                onChange={(event) => setReason(event.target.value)}
                placeholder="kept with the contract"
              />
            </Field>
            {mutation.isError ? (
              <p className="text-sm text-destructive">
                {mutation.error instanceof Error
                  ? mutation.error.message
                  : "Refused."}
              </p>
            ) : null}
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCancelling(false)}>
              Keep it
            </Button>
            <Button
              variant="destructive"
              disabled={mutation.isPending}
              onClick={() =>
                mutation.mutate({
                  path: "cancel",
                  body: {
                    immediately,
                    reason: reason.trim() === "" ? null : reason.trim(),
                  },
                })
              }
            >
              {mutation.isPending ? "Cancelling…" : "Cancel it"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
