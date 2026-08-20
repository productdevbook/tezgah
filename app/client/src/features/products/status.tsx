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
import type { Product, ProductStatus } from "@/api/schemas"

/**
 * The five transitions, and only the ones this status permits.
 *
 * Each is its own route rather than a field on `PATCH`, because the crate
 * moves a status by the transition that is allowed rather than by writing the
 * word — so the panel offers the transitions instead of a dropdown of
 * statuses. A dropdown would let somebody pick `published` from `rejected`
 * and be refused for a reason the screen never mentioned.
 */
const NEXT: Record<ProductStatus, { label: string; path: string }[]> = {
  draft: [
    { label: "Publish", path: "publish" },
    { label: "Submit for review", path: "submit" },
    { label: "Archive", path: "archive" },
  ],
  proposed: [
    { label: "Approve", path: "approve" },
    { label: "Reject", path: "reject" },
  ],
  published: [{ label: "Archive", path: "archive" }],
  rejected: [{ label: "Submit again", path: "submit" }],
  archived: [{ label: "Publish", path: "publish" }],
}

async function move(id: string, path: string, body?: unknown): Promise<void> {
  await apiMutator(`/admin/products/${encodeURIComponent(id)}/${path}`, {
    method: "POST",
    ...(body
      ? {
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        }
      : {}),
  })
}

export function StatusActions({ item }: { item: Product }) {
  const client = useQueryClient()
  const [rejecting, setRejecting] = useState(false)
  const [reason, setReason] = useState("")

  const mutation = useMutation({
    mutationFn: ({ path, body }: { path: string; body?: unknown }) =>
      move(item.id, path, body),
    onSuccess: () => {
      setRejecting(false)
      setReason("")
      void client.invalidateQueries({ queryKey: ["products"] })
    },
  })

  const moves = NEXT[item.status] ?? []
  if (moves.length === 0) return null

  return (
    <>
      <ActionMenu
        groups={[
          moves.map((one) => ({
            label: one.label,
            destructive: one.path === "reject" || one.path === "archive",
            // Rejecting asks why: the reason is stored on the product and
            // read by whoever submitted it, so an empty one helps nobody.
            onSelect: () =>
              one.path === "reject"
                ? setRejecting(true)
                : mutation.mutate({ path: one.path }),
          })),
        ]}
      />
      <Dialog open={rejecting} onOpenChange={setRejecting}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Reject this product</DialogTitle>
            <DialogDescription>
              Whoever submitted it sees the reason, so say what would have to
              change.
            </DialogDescription>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="reject-reason">Reason</FieldLabel>
            <Input
              id="reject-reason"
              value={reason}
              onChange={(event) => setReason(event.target.value)}
            />
          </Field>
          {mutation.isError ? (
            <p className="text-sm text-destructive">
              {mutation.error instanceof Error
                ? mutation.error.message
                : "Refused."}
            </p>
          ) : null}
          <DialogFooter>
            <Button variant="outline" onClick={() => setRejecting(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              disabled={mutation.isPending || reason.trim() === ""}
              onClick={() =>
                mutation.mutate({
                  path: "reject",
                  body: { reason: reason.trim() },
                })
              }
            >
              {mutation.isPending ? "Rejecting…" : "Reject"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}
