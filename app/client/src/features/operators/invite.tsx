import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useState } from "react"
import { useForm, useWatch } from "react-hook-form"

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
import { FormField } from "@/components/form/form"
import {
  invite,
  listInvitations,
  newInvitation,
  role as roleSchema,
  ROLE_MEANS,
  type NewInvitation,
  type Role,
} from "@/features/operators/api"
import { useT } from "@/panel/i18n"

/**
 * Inviting somebody instead of making their account and telling them the
 * password.
 *
 * A server with no mailer refuses this, and says so where the button is
 * rather than in a toast that has gone by the time anybody reads it. The
 * other way still works and is the one a shop without SMTP uses.
 */
export function InviteAction() {
  const t = useT()
  const [open, setOpen] = useState(false)
  const client = useQueryClient()

  // The schema, rather than a second opinion about what an address looks
  // like. `newInvitation` already says it; the hand-written regex that used
  // to sit here was a copy that could disagree with the server's answer and
  // with itself.
  const form = useForm<NewInvitation>({
    resolver: zodResolver(newInvitation),
    defaultValues: { email: "", name: "", role: "staff" },
  })

  const mutation = useMutation({
    mutationFn: (body: NewInvitation) => invite(body),
    onSuccess: () => {
      setOpen(false)
      form.reset()
      void client.invalidateQueries({ queryKey: ["invitations"] })
    },
  })

  // `useWatch` rather than `form.watch()`: the second returns a function the
  // React compiler cannot memoize, and re-renders this on every keystroke of
  // every field rather than of this one.
  const role = useWatch({ control: form.control, name: "role" })

  return (
    <>
      <Button size="sm" variant="outline" onClick={() => setOpen(true)}>
        Invite
      </Button>
      <Dialog
        open={open}
        onOpenChange={(next) => {
          setOpen(next)
          if (!next) form.reset()
        }}
      >
        <DialogContent>
          <form
            onSubmit={form.handleSubmit((values) =>
              mutation.mutateAsync(values)
            )}
          >
            <DialogHeader>
              <DialogTitle>Invite somebody</DialogTitle>
              <DialogDescription>
                They get a link that works once and runs out in seven days. They
                choose their own password, so nobody else ever knows it.
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-3 py-2">
              <FormField
                control={form.control}
                name="email"
                label={t("field.email")}
              >
                {(field) => <Input id={field.name} type="email" {...field} />}
              </FormField>
              <FormField
                control={form.control}
                name="name"
                label={t("field.name")}
              >
                {(field) => <Input id={field.name} {...field} />}
              </FormField>
              <FormField
                control={form.control}
                name="role"
                label={t("field.role")}
              >
                {(field) => (
                  <Select
                    value={field.value}
                    onValueChange={(value) => field.onChange(value as Role)}
                  >
                    <SelectTrigger id={field.name}>
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
                )}
              </FormField>
              <p className="text-xs text-muted-foreground">
                {ROLE_MEANS[role]}
              </p>
              {mutation.isError ? (
                <p className="text-sm text-destructive">
                  {mutation.error instanceof Error
                    ? mutation.error.message
                    : "Refused."}
                </p>
              ) : null}
            </div>
            <DialogFooter>
              <Button
                type="button"
                variant="outline"
                onClick={() => setOpen(false)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={form.formState.isSubmitting}>
                {form.formState.isSubmitting
                  ? "Sending…"
                  : "Send the invitation"}
              </Button>
            </DialogFooter>
          </form>
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
