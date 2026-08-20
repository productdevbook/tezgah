import { QueryClientProvider, type QueryClient } from "@tanstack/react-query"
import { useMemo, type PropsWithChildren } from "react"

import { LocaleContext, type Locale } from "@/panel/i18n"
import { panelQueryClient } from "@/panel/query-client"
import { configurePanel, type PanelConfig } from "@/panel/runtime"

export type PanelProviderProps = PropsWithChildren<
  Partial<PanelConfig> & {
    /**
     * A host that already has one passes it, so the panel's queries share its
     * cache and its devtools rather than opening a second client beside it.
     */
    queryClient?: QueryClient
  }
>

/**
 * What a host wraps the panel's screens in.
 *
 * `configurePanel` is called during render rather than in an effect on
 * purpose: an effect runs after the first render, and by then a screen's
 * first query has already gone to whatever the previous configuration named.
 * It is idempotent, so a re-render costs nothing.
 */
export function PanelProvider({
  children,
  queryClient,
  apiBase,
  token,
  onUnauthenticated,
  locale,
  basepath,
}: PanelProviderProps) {
  configurePanel({
    ...(apiBase !== undefined ? { apiBase } : {}),
    ...(token !== undefined ? { token } : {}),
    ...(onUnauthenticated !== undefined ? { onUnauthenticated } : {}),
    ...(locale !== undefined ? { locale } : {}),
    ...(basepath !== undefined ? { basepath } : {}),
  })

  const client = useMemo(() => queryClient ?? panelQueryClient(), [queryClient])
  const active: Locale = locale ?? "en"

  return (
    <QueryClientProvider client={client}>
      <LocaleContext.Provider value={active}>{children}</LocaleContext.Provider>
    </QueryClientProvider>
  )
}
