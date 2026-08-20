import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { Link } from "@tanstack/react-router"

import { Button } from "@/components/ui/button"

/**
 * `GET /admin/publishable-api-keys` is not bound (`server/README.md`'s route
 * table), so there is nothing here to list — only to mint, at
 * `/store/keys/new`.
 */
export function StoreKeys() {
  return (
    <div className="flex items-center justify-between gap-3 rounded-md border px-4 py-3">
      <div>
        <p className="text-sm font-medium">Mint a publishable key</p>
        <p className="text-xs text-muted-foreground">
          What a storefront sends as <code>x-publishable-key</code>. Shown once.
        </p>
      </div>
      <Button size="sm" nativeButton={false} render={<Link to="/store/keys/new" />}>
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        Mint key
      </Button>
    </div>
  )
}
