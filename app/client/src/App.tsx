import { RouterProvider } from "@tanstack/react-router"
import { useState } from "react"

import { Connect } from "@/components/connect"
import { PanelProvider } from "@/panel/panel-provider"
import { forget, held } from "@/lib/token"
import { router } from "@/router"

/**
 * The standalone panel: this file is the host, and everything it answers is
 * what `panel/runtime.ts` asks any host for. An application mounting these
 * screens inside its own back office writes its own version of this — its
 * address, its session, its locale — and nothing under `features/` changes.
 */
export function App() {
  const [token, setToken] = useState(held)

  // Asked for before anything is drawn rather than after a screen has already
  // said "refused" — the panel knows it holds nothing without a round trip.
  if (!token) return <Connect onConnected={() => setToken(held)} />

  return (
    <PanelProvider
      apiBase={import.meta.env.VITE_TEZGAH_API ?? "/api"}
      token={held}
      onUnauthenticated={() => {
        forget()
        setToken(null)
      }}
      locale="en"
    >
      <RouterProvider router={router} />
    </PanelProvider>
  )
}

export default App
