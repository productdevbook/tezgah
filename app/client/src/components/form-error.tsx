import { ApiError } from "@/api/client"
import { Alert, AlertDescription } from "@/components/ui/alert"

/**
 * The server's `invalid` answers carry one message and no field key
 * (`src/error.rs`'s `Cause::Invalid(String)`), so a failed mutation shows it
 * in the form rather than pretending it points at a particular input.
 *
 * `denied` and `unauthenticated` are told apart from that generic case and
 * from each other, the same distinction `query-state.tsx`'s `Failure` makes
 * for a read: a write this panel's one token was refused for is not "a
 * request that did not go through", and saying so is what a caller checking
 * whether a token split (#214) is needed would look for.
 */
export function FormError({ error }: { error: unknown }) {
  const api = error instanceof ApiError ? error : undefined

  if (api?.kind === "unauthenticated") {
    return (
      <Alert variant="destructive">
        <AlertDescription>
          This panel has no token to send. Connect it with the server's
          ADMIN_TOKEN.
        </AlertDescription>
      </Alert>
    )
  }

  if (api?.kind === "denied") {
    return (
      <Alert variant="destructive">
        <AlertDescription>
          Refused: the token was sent and the server said no. {api.message}
        </AlertDescription>
      </Alert>
    )
  }

  return (
    <Alert variant="destructive">
      <AlertDescription>
        {api?.message ?? "The request did not go through."}
      </AlertDescription>
    </Alert>
  )
}
