import { ApiError } from "@/api/client"
import { Alert, AlertDescription } from "@/components/ui/alert"

/**
 * The server's `invalid` answers carry one message and no field key
 * (`src/error.rs`'s `Cause::Invalid(String)`), so a failed mutation shows it
 * in the form rather than pretending it points at a particular input.
 */
export function FormError({ error }: { error: unknown }) {
  const message =
    error instanceof ApiError
      ? error.message
      : "The request did not go through."
  return (
    <Alert variant="destructive">
      <AlertDescription>{message}</AlertDescription>
    </Alert>
  )
}
