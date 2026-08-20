import type { Locale } from "@/panel/i18n"

/**
 * Everything the panel needs from whoever is running it.
 *
 * The panel is two things at once: the whole application in `app/client`, and
 * — one day — a set of screens mounted inside somebody else's back office.
 * The second only works if nothing below this file reaches for a global: the
 * address of the API, the token to send, what to do when it is refused, and
 * which language to draw in are all the host's answers, not this bundle's.
 *
 * It is a module-level object rather than a React context because the thing
 * that needs it most is not a component. `api/mutator.ts` is called by
 * generated fetch functions, from query functions, outside any tree — a hook
 * cannot reach it. `PanelProvider` writes here on mount, and the context it
 * also provides is for the parts that *are* components.
 */
export type PanelConfig = {
  /** Where `/admin/...` and `/store/...` are served, without a trailing slash. */
  apiBase: string
  /** The bearer token to send, or `null` to send none. Read per request. */
  token: () => string | null
  /** Called when the host answered 401. A standalone panel forgets its token. */
  onUnauthenticated: () => void
  locale: Locale
  /**
   * Where these screens live in the host's URL — `/admin/shop`, or `""` at
   * the root. The router takes it as its basepath; anything that persists
   * something in the browser scopes it by this, so a panel mounted inside an
   * application does not write over what the application keeps.
   */
  basepath: string
}

const standalone: PanelConfig = {
  apiBase: "/api",
  token: () => null,
  onUnauthenticated: () => {},
  locale: "en",
  basepath: "",
}

let current: PanelConfig = standalone

export function configurePanel(config: Partial<PanelConfig>): void {
  current = { ...current, ...config }
}

export function panelRuntime(): PanelConfig {
  return current
}

/**
 * The one thing the panel keeps in the browser by itself, kept here because
 * where it is kept is the host's answer.
 *
 * shadcn's sidebar wrote `sidebar_state` at `path=/`. Mounted inside an
 * application, that writes over whatever the application keeps under the same
 * name, and two panels on one origin write over each other. Scoping it by the
 * basepath is the fix; doing it in this file rather than in the component is
 * what keeps the component from having a second opinion about the browser.
 */
export function rememberSidebar(open: boolean, days = 7): void {
  const { basepath } = panelRuntime()
  const name = basepath
    ? `tezgah_sidebar${basepath.replace(/\W+/g, "_")}`
    : "tezgah_sidebar"
  const age = days * 24 * 60 * 60
  document.cookie = `${name}=${open}; path=${basepath || "/"}; max-age=${age}`
}
