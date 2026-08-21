import type { ReactNode } from "react"

import {
  ExtensionsContext,
  useWidgets,
  type PanelExtensions,
  type WidgetZone,
} from "@/panel/extensions"

/**
 * The two components of the extension seam. They live apart from
 * `extensions.ts` because a file exporting both components and hooks turns
 * off fast refresh for everything in it.
 */
export function ExtensionsProvider({
  extensions,
  children,
}: {
  extensions: PanelExtensions | undefined
  children: ReactNode
}) {
  return (
    <ExtensionsContext.Provider value={extensions ?? {}}>
      {children}
    </ExtensionsContext.Provider>
  )
}

/**
 * Draws whatever the host put in one zone, and nothing when it put nothing.
 *
 * No frame, no heading, no empty state: a zone with no widgets renders
 * nothing at all, so a screen looks exactly as it did before a host existed.
 */
export function Zone({ name, id }: { name: WidgetZone; id?: string }) {
  const mine = useWidgets(name)
  if (mine.length === 0) return null

  return (
    <>
      {mine.map((widget) => (
        <div key={widget.id}>{widget.render({ id })}</div>
      ))}
    </>
  )
}
