import { createContext, useContext, type ReactNode } from "react"

/**
 * What a host may add to these screens, and the whole of it.
 *
 * The panel was mountable before this and not extensible: a host could put
 * every screen under its own prefix and could not add a card to one, or a
 * page of its own beside them. That is the difference between adding commerce
 * to a back office somebody already runs and running two back offices.
 *
 * Two things, deliberately, and no plugin loader: a host composes React
 * already, so an extension is a component it passes in, not a bundle this one
 * fetches and evaluates.
 */

/**
 * Where a widget can go.
 *
 * A closed union rather than a string, so a typo is a compile error instead
 * of a card that renders nowhere. Every name here is rendered by a screen in
 * this bundle — a zone nothing draws would be a promise this file could not
 * keep, so the list grows when a `<Zone/>` is placed, never before.
 */
export type WidgetZone =
  "dashboard" | "product.detail" | "order.detail" | "customer.detail"

/**
 * What the screen tells a widget about where it is.
 *
 * `id` is the row being looked at, and is absent on `dashboard`, which is not
 * about a row. Nothing else is passed: a widget that needs the product needs
 * the API, and it has the same one this panel does.
 */
export type WidgetContext = { id?: string }

export type PanelWidget = {
  zone: WidgetZone
  /** Stable across renders — it is the React key. */
  id: string
  render: (context: WidgetContext) => ReactNode
}

/**
 * A page of the host's own, reachable inside the panel.
 *
 * It appears in the sidebar under the host's name and answers at
 * `<basepath>/<slug>` — the same dynamic route that draws a section this
 * panel has not built, which is why a host screen cannot collide with a built
 * one: a static route always wins the match, so a slug like `orders` is
 * simply never reached and the built screen keeps its address.
 */
export type PanelScreen = {
  slug: string
  /** Drawn as given. The host owns this string, so the host translates it. */
  title: string
  render: () => ReactNode
}

export type PanelExtensions = {
  widgets?: PanelWidget[]
  screens?: PanelScreen[]
  /**
   * The sidebar group the host's screens sit under. Left out, they are
   * grouped under a neutral heading this panel translates.
   */
  groupTitle?: string
}

const EMPTY: PanelExtensions = {}

export const ExtensionsContext = createContext<PanelExtensions>(EMPTY)

export function useHostScreens(): PanelScreen[] {
  return useContext(ExtensionsContext).screens ?? []
}

export function useHostGroupTitle(): string | undefined {
  return useContext(ExtensionsContext).groupTitle
}

export function useHostScreen(slug: string): PanelScreen | undefined {
  return useHostScreens().find((screen) => screen.slug === slug)
}

export function useWidgets(zone: WidgetZone): PanelWidget[] {
  const widgets = useContext(ExtensionsContext).widgets ?? []
  return widgets.filter((widget) => widget.zone === zone)
}
