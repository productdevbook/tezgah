import { createContext, useContext } from "react"

export type RouteModalValue = {
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

/**
 * Apart from the provider that fills it because a file exporting a component
 * may export nothing else and still hot-reload.
 */
export const RouteModalContext = createContext<RouteModalValue | null>(null)

export function useRouteModal(): RouteModalValue {
  const value = useContext(RouteModalContext)
  if (!value) {
    throw new Error("useRouteModal is only available inside a route modal")
  }
  return value
}
