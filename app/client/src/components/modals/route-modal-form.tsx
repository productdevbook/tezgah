import { useBlocker } from "@tanstack/react-router"
import type { ReactNode } from "react"
import {
  FormProvider,
  type FieldValues,
  type UseFormReturn,
} from "react-hook-form"

import { useRouteModal } from "@/components/modals/route-modal-context"
import { useT } from "@/panel/i18n"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"

/**
 * The form inside a route modal, and the one thing every one of them needs:
 * an operator who typed six fields and hit the wrong key does not silently
 * lose them.
 *
 * The guard is on navigation rather than on the modal's own close, because
 * every way out of a route modal is a navigation — the close button, the
 * backdrop, escape, the browser's back button, a link in the form itself. A
 * successful save is the exception, and says so through the context rather
 * than by resetting the form, so a screen that wants to keep what was typed
 * still can.
 */
export function RouteModalForm<TValues extends FieldValues>({
  form,
  onSubmit,
  children,
}: {
  form: UseFormReturn<TValues>
  onSubmit: SubmitHandler<TValues>
  children: ReactNode
}) {
  const t = useT()
  const { submitted } = useRouteModal()

  const blocker = useBlocker({
    shouldBlockFn: () => form.formState.isDirty && !submitted.current,
    withResolver: true,
  })

  return (
    <FormProvider {...form}>
      <form
        onSubmit={form.handleSubmit(onSubmit)}
        className="flex min-h-0 flex-1 flex-col"
      >
        {children}
      </form>
      <AlertDialog
        open={blocker.status === "blocked"}
        onOpenChange={(open) => {
          if (!open) blocker.reset?.()
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{t("general.unsavedTitle")}</AlertDialogTitle>
            <AlertDialogDescription>
              {t("general.unsavedDescription")}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel onClick={() => blocker.reset?.()}>
              {t("actions.cancel")}
            </AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => blocker.proceed?.()}
            >
              {t("actions.continue")}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </FormProvider>
  )
}
