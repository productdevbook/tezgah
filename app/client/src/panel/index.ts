/**
 * What a host imports, and the whole of it.
 *
 * Everything else in this bundle — screens, routes, query keys, the generated
 * client — is this repository's to move. A host that reached past this file
 * would be pinned to shapes nothing promises to keep, so the list is short on
 * purpose: render `<Panel/>`, or wrap your own tree in `<PanelProvider/>` if
 * you are composing the screens yourself.
 *
 * Nothing here reads `import.meta.env` or `localStorage`. The standalone
 * panel's answers to those live in `App.tsx`, which is a host like any other
 * — it just happens to ship in the same repository.
 */
export { Panel, type PanelProps } from "@/panel/mount"
export { PanelProvider, type PanelProviderProps } from "@/panel/panel-provider"
export { configurePanel, panelRuntime, type PanelConfig } from "@/panel/runtime"
export { LOCALES, type Locale } from "@/panel/i18n"
export {
  type PanelExtensions,
  type PanelScreen,
  type PanelWidget,
  type WidgetContext,
  type WidgetZone,
} from "@/panel/extensions"
export { Zone } from "@/panel/zone"
