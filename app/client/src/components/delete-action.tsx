import { useMutation, useQueryClient } from "@tanstack/react-query"
import { useRouter } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { del, type ApiPath } from "@/api/client"
import { FormError } from "@/components/form-error"
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
import { Button } from "@/components/ui/button"

/**
 * One confirm-then-`DELETE` action, shared by every screen `server/README.md`
 * lists a bound `DELETE .../{id}` for. `kind` and `name` go into the dialog's
 * own title so it says what it is about to remove rather than "Are you
 * sure?" — a record it is too late to name once it is gone.
 *
 * The panel holds one bearer token that cannot tell `Action::Delete` apart
 * from `Action::Write` (`CLAUDE.md`, `server/README.md`'s "One token still
 * gates reads and writes alike") — so a `denied` here is a real answer this
 * screen has to show plainly, not a bug to swallow. `FormError` already
 * tells `denied` apart from the rest; nothing here re-decides that.
 */
export function DeleteAction({
  path,
  params,
  invalidateKey,
  kind,
  name,
  onDeleted,
}: {
  path: ApiPath
  params: Record<string, string>
  invalidateKey: unknown[]
  kind: string
  name: string
  onDeleted?: () => void
}) {
  const t = useT()
  const client = useQueryClient()
  const router = useRouter()

  const mutation = useMutation({
    mutationFn: () => del(path, { params }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: invalidateKey })
      // history.back(), not a Link to the list: this is exactly the page
      // and cursor the row was opened from.
      if (onDeleted) onDeleted()
      else router.history.back()
    },
  })

  return (
    <AlertDialog>
      <AlertDialogTrigger render={<Button variant="destructive" size="sm" />}>
        Delete
      </AlertDialogTrigger>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            Delete {kind} "{name}"?
          </AlertDialogTitle>
          <AlertDialogDescription>
            This removes {name} from {kind}s. It cannot be undone from here.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <AlertDialogFooter>
          <AlertDialogCancel>{t("actions.cancel")}</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            disabled={mutation.isPending}
            onClick={() => mutation.mutate()}
          >
            {mutation.isPending ? "Deleting…" : `Delete ${kind}`}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
