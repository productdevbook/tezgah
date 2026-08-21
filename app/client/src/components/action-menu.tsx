import { MoreHorizontalIcon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import type { ReactElement } from "react"

import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { useT } from "@/panel/i18n"

export type Action = {
  label: string
  /** A route to go to. A create or edit form is a route, so most are this. */
  render?: ReactElement
  onSelect?: () => void
  destructive?: boolean
}

/**
 * What a section can have done to it, in one place at its corner.
 *
 * A section with three buttons in its header is three buttons competing with
 * its title for the same line; a section with a menu says the same thing and
 * leaves the title room. Groups are separated, and a destructive action is
 * always in its own group at the bottom.
 */
export function ActionMenu({ groups }: { groups: Action[][] }) {
  const t = useT()
  const visible = groups.filter((group) => group.length > 0)
  if (visible.length === 0) return null

  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label={t("actions.menu")}
          />
        }
      >
        <HugeiconsIcon icon={MoreHorizontalIcon} strokeWidth={2} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {visible.map((group, index) => (
          <div key={index}>
            {index > 0 ? <DropdownMenuSeparator /> : null}
            {group.map((action) => (
              <DropdownMenuItem
                key={action.label}
                variant={action.destructive ? "destructive" : undefined}
                onClick={action.onSelect}
                render={action.render}
              >
                {action.label}
              </DropdownMenuItem>
            ))}
          </div>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
