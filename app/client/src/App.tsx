import { useState } from "react"

import { Connect } from "@/components/connect"
import { Panel } from "@/panel"
import { forget, held } from "@/lib/token"

/**
 * The standalone panel: this file is a host, and everything it answers is what
 * `panel/` asks any host for — the API's address, the token, what to do when
 * one is refused, and where the screens live in the URL.
 *
 * It is deliberately the only file in this bundle that reads
 * `import.meta.env` or `localStorage`. An application mounting these screens
 * inside its own back office writes its own version of this file and imports
 * the same `<Panel/>`; nothing under `features/` changes, and nothing under
 * `features/` can tell which of the two is running it.
 */
export function App() {
  const [token, setToken] = useState(held)

  // Asked for before anything is drawn rather than after a screen has already
  // said "refused" — the panel knows it holds nothing without a round trip.
  if (!token) return <Connect onConnected={() => setToken(held)} />

  return (
    <Panel
      apiBase={import.meta.env.VITE_TEZGAH_API ?? "/api"}
      token={held}
      onUnauthenticated={() => {
        forget()
        setToken(null)
      }}
      locale="en"
    />
  )
}

export default App
