const KEY = "tezgah.admin-token"

/**
 * The admin token, held in this browser and nowhere else.
 *
 * tezgah authenticates nobody — `docs/hosting.md` — and the server this panel
 * talks to gates `/admin/*` on a bearer token it was started with. So there is
 * no session to establish and nothing to log in to: the panel holds the token
 * an operator pastes in, and sends it. `localStorage` rather than a cookie
 * because nothing here is same-origin by necessity and a cookie would be sent
 * to the storefront routes too.
 */
export function held(): string | null {
  try {
    return localStorage.getItem(KEY)
  } catch {
    return null
  }
}

export function hold(token: string): void {
  localStorage.setItem(KEY, token.trim())
}

export function forget(): void {
  localStorage.removeItem(KEY)
}
