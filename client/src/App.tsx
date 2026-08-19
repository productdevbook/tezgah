import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"

import { ApiError } from "@/api/client"
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
  return (
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
    </QueryClientProvider>
  )
}

export default App
