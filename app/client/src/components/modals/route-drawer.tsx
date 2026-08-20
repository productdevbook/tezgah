import type { PropsWithChildren, ReactNode } from "react"

import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalProvider } from "@/components/modals/route-modal"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetTitle,
} from "@/components/ui/sheet"

/**
 * A form that changes one part of a record it is standing on: a product's
 * organisation, a region's countries.
 *
 * A drawer rather than a page because the record stays visible behind it —
 * the operator can still read what they are changing it from. Like the focus
 * modal it is a route, so it has its own address and closing it goes back.
 */
export function RouteDrawer({
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
    <Sheet
      open
      onOpenChange={(open) => {
        if (!open) close()
      }}
    >
      <SheetContent
        side="right"
        className="flex w-full flex-col gap-0 p-0 sm:max-w-md"
      >
        {children}
      </SheetContent>
    </Sheet>
  )
}

RouteDrawer.Header = function Header({
  title,
  description,
}: {
  title: string
  description?: string
}) {
  return (
    <header className="shrink-0 border-b px-6 py-4">
      <SheetTitle className="truncate">{title}</SheetTitle>
      {description ? (
        <SheetDescription className="mt-0.5">{description}</SheetDescription>
      ) : null}
    </header>
  )
}

RouteDrawer.Body = function DrawerBody({ children }: PropsWithChildren) {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">{children}</div>
  )
}

RouteDrawer.Footer = function Footer({ children }: { children: ReactNode }) {
  return (
    <footer className="flex shrink-0 items-center justify-end gap-2 border-t px-6 py-4">
      {children}
    </footer>
  )
}
