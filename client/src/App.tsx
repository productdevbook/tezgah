import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"
import { useState } from "react"

import { ApiError } from "@/api/client"
import { Connect } from "@/components/connect"
import { held } from "@/lib/token"
import { router } from "@/router"

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      /**
       * A refused request is an answer, not a hiccup — retrying it asks the
       * host to say no four more times. Only an unreachable host is worth
       * trying again.
       */
      retry: (attempt, error) =>
        error instanceof ApiError && error.kind === "unreachable" && attempt < 2,
    },
  },
})

export function App() {
  const [token, setToken] = useState(held)

  // Asked for before anything is drawn rather than after a screen has already
  // said "refused" — the panel knows it holds nothing without a round trip.
  if (!token) return <Connect onConnected={() => setToken(held)} />

  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  )
}

export default App
