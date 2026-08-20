import { useState } from "react"

import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { hold } from "@/lib/token"

/**
 * Not a login. tezgah authenticates nobody, and the server this panel talks to
 * gates `/admin/*` on the one bearer token it was started with — so there is
 * no account here, no session, and nothing to reset. An operator pastes the
 * token the server was given; the browser keeps it.
 */
export function Connect({ onConnected }: { onConnected: () => void }) {
  const [token, setToken] = useState("")

  return (
    <div className="flex min-h-svh items-center justify-center p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>Connect this panel</CardTitle>
          <CardDescription>
            The admin token the server was started with — its `ADMIN_TOKEN`.
            It stays in this browser and is sent on every admin request.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form
            className="space-y-3"
            onSubmit={(event) => {
              event.preventDefault()
              if (!token.trim()) return
              hold(token)
              onConnected()
            }}
          >
            <div className="space-y-1.5">
              <Label htmlFor="token">Admin token</Label>
              <Input
                id="token"
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={token}
                onChange={(event) => setToken(event.target.value)}
                placeholder="the value of ADMIN_TOKEN"
              />
            </div>
            <Button type="submit" className="w-full" disabled={!token.trim()}>
              Connect
            </Button>
            <p className="text-muted-foreground text-xs">
              Started without one and the server serves no admin surface at all,
              so there is nothing a token could reach.
            </p>
          </form>
        </CardContent>
      </Card>
    </div>
  )
}
