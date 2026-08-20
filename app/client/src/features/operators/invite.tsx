import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useState } from "react"

import { TableFrame } from "@/components/data-table"
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Badge } from "@/components/ui/badge"
import { dateTime } from "@/lib/detail"
import {
  invite,
  listInvitations,
  role as roleSchema,
  ROLE_MEANS,
  type Role,
} from "@/features/operators/api"

/**
 * Inviting somebody instead of making their account and telling them the
 * password.
 *
 * A server with no mailer refuses this, and says so where the button is
 * rather than in a toast that has gone by the time anybody reads it. The
 * other way still works and is the one a shop without SMTP uses.
 */
export function InviteAction() {
  const [open, setOpen] = useState(false)
  const [email, setEmail] = useState("")
  const [name, setName] = useState("")
  const [role, setRole] = useState<Role>("staff")
  const client = useQueryClient()

  const mutation = useMutation({
    mutationFn: () => invite({ email: email.trim(), name: name.trim(), role }),
    onSuccess: () => {
      setOpen(false)
      setEmail("")
      setName("")
      void client.invalidateQueries({ queryKey: ["invitations"] })
    },
  })

  const looksLikeAddress = /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email.trim())

  return (
    <>
      <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
        Invite
      </Button>
      <Dialog open={open} onOpenChange={setOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Invite somebody</DialogTitle>
            <DialogDescription>
              They get a link that works once and runs out in seven days. They
              choose their own password, so nobody else ever knows it.
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <Field>
              <FieldLabel htmlFor="invite-email">E-mail</FieldLabel>
              <Input
                id="invite-email"
                type="email"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                aria-invalid={email.length > 0 && !looksLikeAddress}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="invite-name">Name</FieldLabel>
              <Input
                id="invite-name"
                value={name}
                onChange={(event) => setName(event.target.value)}
              />
            </Field>
            <Field>
              <FieldLabel htmlFor="invite-role">Role</FieldLabel>
              <Select
                value={role}
                onValueChange={(value) => setRole(value as Role)}
              >
                <SelectTrigger id="invite-role">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {roleSchema.options.map((one) => (
                    <SelectItem key={one} value={one}>
                      {one}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {ROLE_MEANS[role]}
              </p>
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
            <Button variant="outline" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button
              onClick={() => mutation.mutate()}
              disabled={
                mutation.isPending || !looksLikeAddress || name.trim() === ""
              }
            >
              {mutation.isPending ? "Sending…" : "Send the invitation"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  )
}

/**
 * Who has been invited and has not arrived.
 *
 * No token here, and there cannot be one: the server kept a digest. An owner
 * who needs to resend invites again, which replaces the open invitation
 * rather than adding a second.
 */
export function OpenInvitations() {
  const result = useQuery({
    queryKey: ["invitations"],
    queryFn: ({ signal }) => listInvitations(signal),
    // A server with no mailer refuses this too, and an empty list is the
    // honest way to draw that rather than an error beside the accounts.
    retry: false,
  })

  const rows = result.data ?? []
  if (rows.length === 0) return null

  return (
    <TableFrame
      header={{
        title: "Invited",
        description:
          "Sent and not yet accepted. Inviting the same address again replaces the link rather than adding a second.",
      }}
    >
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>E-mail</TableHead>
            <TableHead>Name</TableHead>
            <TableHead>Role</TableHead>
            <TableHead className="text-right">Runs out</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={row.id}>
              <TableCell>{row.email}</TableCell>
              <TableCell>{row.name}</TableCell>
              <TableCell>
                <Badge variant="outline">{row.role}</Badge>
              </TableCell>
              <TableCell className="text-right text-xs text-muted-foreground">
                {dateTime(row.expires_at)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </TableFrame>
  )
}
