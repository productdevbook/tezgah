import { useState, type FormEvent } from "react"

import { apiMutator } from "@/api/mutator"
import { ApiError } from "@/api/errors"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Field, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { acceptInvitation } from "@/features/operators/api"
import { hold } from "@/lib/token"
import { useT } from "@/panel/i18n"

/**
 * Two ways in, because the server has two.
 *
 * An operator signs in and gets a session that expires and can be revoked.
 * `ADMIN_TOKEN` is the shared secret that makes the first account and gets
 * back in when the last password is lost — it is not a person, and the panel
 * says so rather than dressing it up as one.
 *
 * Either way what the browser keeps is a bearer token, so nothing past this
 * screen has to know which it holds.
 */
export function Connect({
  onConnected,
  invitation,
}: {
  onConnected: () => void
  /**
   * The token out of `?invitation=…`, read by `App.tsx` — the host, and the
   * only file here allowed to look at the URL.
   */
  invitation?: string
}) {
  const t = useT()
  // An invitation is not a third tab. Somebody arriving on that link has no
  // account and no token, so offering them two ways to sign in with neither
  // is offering them nothing.
  if (invitation) {
    return (
      <div className="flex min-h-svh items-center justify-center p-6">
        <Card className="w-full max-w-md">
          <CardHeader>
            <CardTitle>{t("connect.choosePassword")}</CardTitle>
            <CardDescription>{t("connect.choosePasswordWhy")}</CardDescription>
          </CardHeader>
          <CardContent>
            <Accept token={invitation} onConnected={onConnected} />
          </CardContent>
        </Card>
      </div>
    )
  }

  return (
    <div className="flex min-h-svh items-center justify-center p-6">
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>{t("connect.signIn")}</CardTitle>
          <CardDescription>{t("connect.signInWhy")}</CardDescription>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="account">
            <TabsList className="mb-4">
              <TabsTrigger value="account">{t("connect.account")}</TabsTrigger>
              <TabsTrigger value="token">{t("connect.adminToken")}</TabsTrigger>
            </TabsList>
            <TabsContent value="account">
              <WithPassword onConnected={onConnected} />
            </TabsContent>
            <TabsContent value="token">
              <WithToken onConnected={onConnected} />
            </TabsContent>
          </Tabs>
        </CardContent>
      </Card>
    </div>
  )
}

function Accept({
  token,
  onConnected,
}: {
  token: string
  onConnected: () => void
}) {
  const t = useT()
  const [password, setPassword] = useState("")
  const [again, setAgain] = useState("")
  const [refused, setRefused] = useState<string | null>(null)
  const [sending, setSending] = useState(false)

  const short = password.length > 0 && password.length < 12
  const differs = again.length > 0 && again !== password

  async function submit(event: FormEvent) {
    event.preventDefault()
    setRefused(null)
    setSending(true)
    try {
      // Straight in: accepting answers with a session, because somebody who
      // just proved they hold the invitation and chose the password should not
      // be sent to a form to type it again.
      const session = await acceptInvitation(token, password)
      hold(session.token)
      onConnected()
    } catch (error) {
      setRefused(
        error instanceof ApiError && error.kind === "unreachable"
          ? t("connect.noHostAnswered")
          : t("connect.invitationSpent")
      )
    } finally {
      setSending(false)
    }
  }

  return (
    <form className="space-y-3" onSubmit={submit}>
      <Field>
        <FieldLabel htmlFor="new-password">{t("connect.password")}</FieldLabel>
        <Input
          id="new-password"
          type="password"
          autoComplete="new-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
          aria-invalid={short}
        />
        {short ? (
          <p className="text-xs text-destructive">{t("connect.tooShort")}</p>
        ) : null}
      </Field>
      <Field>
        <FieldLabel htmlFor="again">{t("connect.again")}</FieldLabel>
        <Input
          id="again"
          type="password"
          autoComplete="new-password"
          value={again}
          onChange={(event) => setAgain(event.target.value)}
          aria-invalid={differs}
        />
        {differs ? (
          <p className="text-xs text-destructive">{t("connect.doNotMatch")}</p>
        ) : null}
      </Field>
      {refused ? <p className="text-sm text-destructive">{refused}</p> : null}
      <Button
        type="submit"
        className="w-full"
        disabled={sending || short || differs || password.length < 12 || !again}
      >
        {sending ? t("connect.setting") : t("connect.setAndSignIn")}
      </Button>
    </form>
  )
}

function WithPassword({ onConnected }: { onConnected: () => void }) {
  const t = useT()
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [refused, setRefused] = useState<string | null>(null)
  const [sending, setSending] = useState(false)

  async function submit(event: FormEvent) {
    event.preventDefault()
    setRefused(null)
    setSending(true)
    try {
      const { data } = await apiMutator<{ data: { token: string } }>(
        "/auth/session",
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ email, password }),
        }
      )
      hold(data.token)
      onConnected()
    } catch (error) {
      // The server answers the same way to an address nobody holds and a
      // password that is wrong, on purpose. So does this.
      setRefused(
        error instanceof ApiError && error.kind === "unreachable"
          ? t("connect.noHostAnswered")
          : t("connect.wrongPassword")
      )
    } finally {
      setSending(false)
    }
  }

  return (
    <form className="space-y-3" onSubmit={submit}>
      <Field>
        <FieldLabel htmlFor="email">{t("connect.email")}</FieldLabel>
        <Input
          id="email"
          type="email"
          autoComplete="username"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
      </Field>
      <Field>
        <FieldLabel htmlFor="password">{t("connect.password")}</FieldLabel>
        <Input
          id="password"
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
      </Field>
      {refused ? <p className="text-sm text-destructive">{refused}</p> : null}
      <Button
        type="submit"
        className="w-full"
        disabled={sending || !email || !password}
      >
        {sending ? t("connect.signingIn") : t("connect.signIn")}
      </Button>
      <p className="text-xs text-muted-foreground">
        {t("connect.noSelfReset")}
      </p>
    </form>
  )
}

function WithToken({ onConnected }: { onConnected: () => void }) {
  const t = useT()
  const [token, setToken] = useState("")

  return (
    <form
      className="space-y-3"
      onSubmit={(event) => {
        event.preventDefault()
        if (!token.trim()) return
        hold(token)
        onConnected()
      }}
    >
      <Field>
        <FieldLabel htmlFor="token">{t("connect.adminToken")}</FieldLabel>
        <Input
          id="token"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={token}
          onChange={(event) => setToken(event.target.value)}
          placeholder={t("connect.tokenPlaceholder")}
        />
      </Field>
      <Button type="submit" className="w-full" disabled={!token.trim()}>
        {t("connect.connect")}
      </Button>
      <p className="text-xs text-muted-foreground">{t("connect.tokenWhy")}</p>
    </form>
  )
}
