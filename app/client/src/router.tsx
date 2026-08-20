import { createRouter } from "@tanstack/react-router"

import { routeTree } from "./routeTree.gen"

/**
 * One router per mount, because a basepath is a mount's answer.
 *
 * The panel used to export a single router built at module load, which meant
 * its routes could only ever live at the root of an origin. A host mounting
 * these screens under `/admin/shop` needs the same tree rooted there, and a
 * host that mounts two of them — a staging shop beside a live one — needs two
 * routers, not one with a mutable prefix.
 *
 * `basepath` is the only thing that varies. Everything else a host answers
 * goes through `panel/runtime.ts`, and the screens under `features/` know
 * about neither.
 */
export function createPanelRouter(basepath?: string) {
  return createRouter({ routeTree, ...(basepath ? { basepath } : {}) })
}

/**
 * The standalone panel's own router, and the one the type registration below
 * is written against. Every router this module makes has the same type, so
 * `Link`'s `to` stays checked against the same route tree wherever the panel
 * is mounted.
 */
export const router = createPanelRouter()

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
