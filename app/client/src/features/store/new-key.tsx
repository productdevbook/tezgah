import { zodResolver } from "@hookform/resolvers/zod"
import { useMutation, useQueryClient } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Copy01Icon, Tick02Icon } from "@hugeicons/core-free-icons"
import { useNavigate } from "@tanstack/react-router"
import { useState } from "react"
import { useForm } from "react-hook-form"
import type { z } from "zod"

import { post } from "@/api/client"
import {
  createPublishableKey,
  issuedKey,
  type CreatePublishableKey,
  type IssuedKey,
} from "@/api/schemas"
import { FormField } from "@/components/form/form"
import { FormError } from "@/components/form-error"
import { RouteFocusModal } from "@/components/modals/route-focus-modal"
import { useRouteModal } from "@/components/modals/route-modal-context"
import { RouteModalForm } from "@/components/modals/route-modal-form"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useT } from "@/panel/i18n"

type Fields = z.infer<typeof createPublishableKey>

export function NewPublishableKey() {
  const navigate = useNavigate()

  return (
    <RouteFocusModal onClose={() => void navigate({ to: "/store/keys" })}>
      <Body />
    </RouteFocusModal>
  )
}

/**
 * The raw token comes back once (`store::create_publishable_key`'s own doc
 * comment) and is never stored anywhere it could be read back — so, unlike
 * every other creation form here, this one does not leave on success. It
 * shows the token in place and waits for "Done", which is the moment the
 * token stops being reachable either way.
 */
function Body() {
  const t = useT()
  const client = useQueryClient()
  const { close, succeed, markSaved } = useRouteModal()
  const [issued, setIssued] = useState<IssuedKey | null>(null)

  const form = useForm<Fields>({
    resolver: zodResolver(createPublishableKey),
    defaultValues: { title: "" },
  })

  const mutation = useMutation({
    mutationFn: (body: CreatePublishableKey) =>
      post("/admin/publishable-api-keys", { schema: issuedKey, body }),
    onSuccess: (key) => {
      void client.invalidateQueries({ queryKey: ["publishable-keys"] })
      // The guard is silenced here rather than on the way out: what is on
      // screen after this is the token, and being asked whether to discard
      // an already-minted key would be a question with no true answer.
      markSaved()
      setIssued(key)
    },
  })

  if (issued) return <Issued issued={issued} onDone={succeed} />

  return (
    <RouteModalForm
      form={form}
      onSubmit={(values) => mutation.mutateAsync(values)}
    >
      <RouteFocusModal.Header
        title={t("form.key.title")}
        description={t("form.key.why")}
      />
      <RouteFocusModal.Body>
        <div className="mx-auto flex w-full max-w-xl flex-col gap-6">
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <FormField
            control={form.control}
            name="title"
            label={t("field.title")}
          >
            {(field) => (
              <Input id={field.name} placeholder="Storefront" {...field} />
            )}
          </FormField>
        </div>
      </RouteFocusModal.Body>
      <RouteFocusModal.Footer>
        <Button type="button" variant="outline" onClick={close}>
          {t("actions.cancel")}
        </Button>
        <Button type="submit" disabled={form.formState.isSubmitting}>
          {form.formState.isSubmitting ? "Minting…" : "Mint key"}
        </Button>
      </RouteFocusModal.Footer>
    </RouteModalForm>
  )
}

function Issued({ issued, onDone }: { issued: IssuedKey; onDone: () => void }) {
  const t = useT()
  const [copied, setCopied] = useState(false)

  return (
    <>
      <RouteFocusModal.Header
        title={t("form.key.copyNow")}
        description={t("form.key.copyNowWhy")}
      />
      <RouteFocusModal.Body>
        <div className="mx-auto w-full max-w-xl space-y-2 rounded-md border border-primary/30 bg-primary/5 px-4 py-3">
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
              <HugeiconsIcon
                icon={copied ? Tick02Icon : Copy01Icon}
                strokeWidth={2}
              />
              <span className="sr-only">Copy</span>
            </Button>
          </div>
        </div>
      </RouteFocusModal.Body>
      <RouteFocusModal.Footer>
        <Button type="button" onClick={onDone}>
          Done
        </Button>
      </RouteFocusModal.Footer>
    </>
  )
}
