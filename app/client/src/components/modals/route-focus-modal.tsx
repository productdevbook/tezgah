import type { PropsWithChildren, ReactNode } from "react"

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog"
import {
  RouteModalProvider,
  useRouteModal,
} from "@/components/modals/route-modal"
import { cn } from "@/lib/utils"

/**
 * A form that wants the screen: creating a product, importing a file.
 *
 * Near-fullscreen rather than a small dialog, because a creation form with
 * six sections in a 400-pixel box is a scroll bar with a title. The page it
 * was opened from is still behind it and still where closing lands.
 */
export function RouteFocusModal({
  onClose,
  children,
}: PropsWithChildren<{ onClose?: () => void }>) {
  return (
    <RouteModalProvider onClose={onClose}>
      <Body>{children}</Body>
    </RouteModalProvider>
  )
}

function Body({ children }: PropsWithChildren) {
  const { close } = useRouteModal()

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) close()
      }}
    >
      <DialogContent
        className={cn(
          "flex h-[calc(100svh-2rem)] w-[calc(100vw-2rem)] max-w-5xl flex-col gap-0 overflow-hidden p-0",
          "sm:max-w-5xl"
        )}
      >
        {children}
      </DialogContent>
    </Dialog>
  )
}

RouteFocusModal.Header = function Header({
  title,
  description,
  actions,
}: {
  title: string
  description?: string
  actions?: ReactNode
}) {
  return (
    <header className="flex shrink-0 items-center justify-between gap-4 border-b px-6 py-4">
      <div className="min-w-0">
        <DialogTitle className="truncate text-base font-medium">
          {title}
        </DialogTitle>
        {description ? (
          <DialogDescription className="mt-0.5">
            {description}
          </DialogDescription>
        ) : null}
      </div>
      {actions ? (
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      ) : null}
    </header>
  )
}

RouteFocusModal.Body = function ModalBody({ children }: PropsWithChildren) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">{children}</div>
  )
}

RouteFocusModal.Footer = function Footer({ children }: PropsWithChildren) {
  return (
    <footer className="flex shrink-0 items-center justify-end gap-2 border-t px-6 py-4">
      {children}
    </footer>
  )
}
