import { useState, type FormEvent } from "react"
import { useMutation } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Copy01Icon, Tick02Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"

import { post } from "@/api/client"
import {
  createPublishableKey,
  issuedKey,
  type CreatePublishableKey,
  type IssuedKey,
} from "@/api/schemas"
import { FormError } from "@/components/form-error"
import { PageHeading } from "@/components/page-heading"
import { Button } from "@/components/ui/button"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"

/**
 * The raw token comes back once (`store::create_publishable_key`'s own doc
 * comment) and is never stored anywhere it could be read back — so, unlike
 * the other four creation routes, this one does not navigate away on
 * success. It shows the token in place and waits for "Done" before
 * returning to `/store/keys`, the same moment the token stops being
 * reachable either way.
 */
export function NewPublishableKey() {
  const [title, setTitle] = useState("")
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [issued, setIssued] = useState<IssuedKey | null>(null)

  const mutation = useMutation({
    mutationFn: (body: CreatePublishableKey) =>
      post("/admin/publishable-api-keys", { schema: issuedKey, body }),
    onSuccess: (key) => setIssued(key),
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createPublishableKey.safeParse({ title })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  if (issued) return <IssuedKeyCard issued={issued} />

  return (
    <div className="max-w-xl space-y-4">
      <PageHeading
        title="Mint a publishable key"
        subtitle="The token is shown once, right after this. Losing it means minting another."
      />
      <form className="space-y-4" onSubmit={submit}>
        {mutation.isError ? <FormError error={mutation.error} /> : null}
        <Field data-invalid={!!fieldErrors.title}>
          <FieldLabel htmlFor="key-title">Title</FieldLabel>
          <Input
            id="key-title"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            placeholder="Storefront"
            aria-invalid={!!fieldErrors.title}
          />
          <FieldError>{fieldErrors.title}</FieldError>
        </Field>
        <div className="flex items-center gap-2">
          <Button type="submit" disabled={mutation.isPending}>
            {mutation.isPending ? "Minting…" : "Mint key"}
          </Button>
          <Button
            type="button"
            variant="outline"
            nativeButton={false}
            render={<Link to="/store/keys" />}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  )
}

function IssuedKeyCard({ issued }: { issued: IssuedKey }) {
  const [copied, setCopied] = useState(false)

  return (
    <div className="max-w-xl space-y-4">
      <PageHeading title="Copy this key now" subtitle="This will not be shown again." />
      <div className="space-y-2 rounded-md border border-primary/30 bg-primary/5 px-4 py-3">
        <p className="text-sm font-medium">{issued.title}</p>
        <div className="flex items-center gap-2">
          <code className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1.5 text-xs">
            {issued.token}
          </code>
          <Button
            type="button"
            size="icon-sm"
            variant="outline"
            onClick={() => {
              void navigator.clipboard.writeText(issued.token).then(() => {
                setCopied(true)
                setTimeout(() => setCopied(false), 2000)
              })
            }}
          >
            <HugeiconsIcon icon={copied ? Tick02Icon : Copy01Icon} strokeWidth={2} />
            <span className="sr-only">Copy</span>
          </Button>
        </div>
      </div>
      <Button nativeButton={false} render={<Link to="/store/keys" />}>
        Done
      </Button>
    </div>
  )
}
