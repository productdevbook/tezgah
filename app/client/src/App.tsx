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
/**
 * Read once, at load. An invitation's token arrives in the URL because a
 * letter cannot carry anything else, and this is the host reading the host's
 * address bar — the one file allowed to.
 */
function invitationFromUrl(): string | undefined {
  try {
    return (
      new URLSearchParams(window.location.search).get("invitation") ?? undefined
    )
  } catch {
    return undefined
  }
}

export function App() {
  const [token, setToken] = useState(held)
  const [invitation] = useState(invitationFromUrl)

  // Asked for before anything is drawn rather than after a screen has already
  // said "refused" — the panel knows it holds nothing without a round trip.
  if (!token) {
    return (
      <Connect
        invitation={invitation}
        onConnected={() => {
          // The token is spent. Leaving it in the address bar leaves it in
          // the history, the referrer and whatever the browser syncs.
          window.history.replaceState(null, "", window.location.pathname)
          setToken(held)
        }}
      />
    )
  }

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
