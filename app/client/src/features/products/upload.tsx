import { useMutation } from "@tanstack/react-query"
import { useRef } from "react"
import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"
import { ApiError } from "@/api/errors"
import { Button } from "@/components/ui/button"

const stored = z.object({ url: z.string() })

/**
 * `POST /admin/files`, when the server has somewhere to put one.
 *
 * A server started without `TEZGAH_FILE_DIR` does not bind this route at all,
 * so a 404 here means "this shop stores no files" rather than "something went
 * wrong" — which is why the button says so instead of showing an error.
 */
async function upload(file: File): Promise<string> {
  const body = new FormData()
  body.append("file", file)

  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    "/admin/files",
    { method: "POST", body }
  )
  return parseResponse(stored, data, status).url
}

/** The five the server stores. Anything else is refused there, and the file
 *  picker not offering it is the kinder half of the same rule. */
const ACCEPT = "image/jpeg,image/png,image/webp,image/gif,image/avif"

export function UploadImage({ onStored }: { onStored: (url: string) => void }) {
  const input = useRef<HTMLInputElement>(null)

  const mutation = useMutation({
    mutationFn: (file: File) => upload(file),
    onSuccess: onStored,
  })

  const unavailable =
    mutation.error instanceof ApiError && mutation.error.status === 404

  return (
    <div className="space-y-2">
      <input
        ref={input}
        type="file"
        accept={ACCEPT}
        className="hidden"
        onChange={(event) => {
          const file = event.target.files?.[0]
          // Cleared either way, so choosing the same file twice after a
          // failure still fires a change.
          event.target.value = ""
          if (file) mutation.mutate(file)
        }}
      />
      <Button
        type="button"
        variant="outline"
        size="sm"
        onClick={() => input.current?.click()}
        disabled={mutation.isPending}
      >
        {mutation.isPending ? "Uploading…" : "Upload an image"}
      </Button>
      {unavailable ? (
        <p className="text-xs text-muted-foreground">
          This shop stores no files — it was started without a file directory.
          Paste an address instead.
        </p>
      ) : mutation.isError ? (
        <p className="text-xs text-destructive">
          {mutation.error instanceof Error
            ? mutation.error.message
            : "That upload was refused."}
        </p>
      ) : null}
    </div>
  )
}
