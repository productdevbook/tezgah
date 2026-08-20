import { useRouter } from "@tanstack/react-router"
import { useCallback, useMemo, useRef, type PropsWithChildren } from "react"

import { RouteModalContext } from "@/components/modals/route-modal-context"

/**
 * The thing a create or edit form is rendered inside.
 *
 * A form here is a route — `/products/new` is an address somebody can send —
 * drawn over the list it came from rather than replacing it. That is the
 * whole point: an operator who opens a form has not lost their place, and a
 * form that saves lands back on the row it changed.
 *
 * `onClose` defaults to going back, which is right when the modal was reached
 * by a link from the page underneath. A form reached directly — pasted
 * address, a reload — has nothing behind it, so a screen that knows where it
 * belongs passes a navigate of its own.
 */
export function RouteModalProvider({
  onClose,
  children,
}: PropsWithChildren<{ onClose?: () => void }>) {
  const router = useRouter()
  const submitted = useRef(false)

  const close = useCallback(() => {
    if (onClose) {
      onClose()
      return
    }
    router.history.back()
  }, [onClose, router])

  const markSaved = useCallback(() => {
    submitted.current = true
  }, [])

  const succeed = useCallback(() => {
    markSaved()
    close()
  }, [close, markSaved])

  const value = useMemo(
    () => ({ close, succeed, markSaved, submitted }),
    [close, succeed, markSaved]
  )

  return (
    <RouteModalContext.Provider value={value}>
      {children}
    </RouteModalContext.Provider>
  )
}
