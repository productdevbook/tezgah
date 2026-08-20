import { useRouter } from "@tanstack/react-router"
import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useRef,
  type PropsWithChildren,
} from "react"

type RouteModalValue = {
  /** Leave the modal the way its close button does. */
  close: () => void
  /**
   * Leave after a successful write. Distinct from `close` because it also
   * tells the unsaved-changes guard the form is no longer dirty in any sense
   * that matters — a saved form must not ask whether to discard.
   */
  succeed: () => void
  /**
   * The half of `succeed` that only silences the guard, for a screen that
   * saves and then goes somewhere the modal itself does not know about — a
   * creation form landing on the record it just made.
   */
  markSaved: () => void
  /** Read by the guard; never set from outside. */
  submitted: { current: boolean }
}

const RouteModalContext = createContext<RouteModalValue | null>(null)

export function useRouteModal(): RouteModalValue {
  const value = useContext(RouteModalContext)
  if (!value) {
    throw new Error("useRouteModal is only available inside a route modal")
  }
  return value
}

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
