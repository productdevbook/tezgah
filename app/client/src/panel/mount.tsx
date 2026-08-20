import { RouterProvider } from "@tanstack/react-router"
import { useMemo } from "react"

import { PanelProvider, type PanelProviderProps } from "@/panel/panel-provider"
import { createPanelRouter } from "@/router"

export type PanelProps = Omit<PanelProviderProps, "children">

/**
 * The whole panel as one element: what a host renders.
 *
 * This is the other half of the seam `panel/runtime.ts` opened. That one
 * answered what a screen *says and sends* — the API's address, the token, the
 * language. This answers where it *lives*, which was the half still fixed: a
 * router built at module load can only be at the root of an origin.
 *
 * A host renders `<Panel basepath="/admin/shop" apiBase="/api/commerce"
 * token={...} onUnauthenticated={...} />` inside its own layout and gets the
 * whole route tree under that prefix. It does not import a screen, a route or
 * a query key, and nothing under `features/` learns that it was mounted.
 *
 * The router is made once per basepath. Rebuilding it on every render would
 * throw away the history and the match state, so a keystroke in a filter
 * would put the screen back at its first page.
 */
export function Panel({ basepath, ...config }: PanelProps) {
  const router = useMemo(() => createPanelRouter(basepath), [basepath])

  return (
    <PanelProvider basepath={basepath} {...config}>
      <RouterProvider router={router} />
    </PanelProvider>
  )
}
