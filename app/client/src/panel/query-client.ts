import { QueryClient } from "@tanstack/react-query"

import { ApiError } from "@/api/errors"

/**
 * The panel's own client, for a host that has none to lend it.
 */
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
