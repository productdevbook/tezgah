import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { useMemo, type PropsWithChildren } from "react"

import { ApiError } from "@/api/errors"
import { LocaleContext, type Locale } from "@/panel/i18n"
import { configurePanel, type PanelConfig } from "@/panel/runtime"

export function panelQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        staleTime: 30_000,
        /**
         * A refused request is an answer, not a hiccup — retrying it asks the
         * host to say no four more times. Only an unreachable host is worth
         * trying again.
         */
        retry: (attempt, error) =>
          error instanceof ApiError &&
          error.kind === "unreachable" &&
          attempt < 2,
      },
    },
  })
}

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
}: PanelProviderProps) {
  configurePanel({
    ...(apiBase !== undefined ? { apiBase } : {}),
    ...(token !== undefined ? { token } : {}),
    ...(onUnauthenticated !== undefined ? { onUnauthenticated } : {}),
    ...(locale !== undefined ? { locale } : {}),
  })

  const client = useMemo(() => queryClient ?? panelQueryClient(), [queryClient])
  const active: Locale = locale ?? "en"

  return (
    <QueryClientProvider client={client}>
      <LocaleContext.Provider value={active}>{children}</LocaleContext.Provider>
    </QueryClientProvider>
  )
}
