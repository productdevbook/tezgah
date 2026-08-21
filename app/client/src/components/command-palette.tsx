import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { GROUPS } from "@/lib/nav"
import { useT } from "@/panel/i18n"
import { useSectionNavigate } from "@/lib/section-navigate"

/**
 * ⌘K, and the only way to reach a section without the sidebar.
 *
 * It searches what the panel *has* — sixteen sections and the two that are
 * folded into other screens — and nothing the shop holds: there is no route
 * that searches products, orders or customers, so a palette offering to would
 * be promising something the API cannot answer. When one arrives, this is
 * where it goes.
 */
export function CommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const goTo = useSectionNavigate()
  const t = useT()

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      <Command>
        <CommandInput placeholder={t("nav.goTo")} />
        <CommandList>
          <CommandEmpty>{t("general.empty")}</CommandEmpty>
          {GROUPS.map((group) => (
            <CommandGroup key={group.title} heading={t(group.title)}>
              {group.sections
                .filter((section) => !section.folded)
                .map((section) => (
                  <CommandItem
                    key={section.slug}
                    value={`${t(group.title)} ${t(section.title)} ${section.slug}`}
                    onSelect={() => {
                      onOpenChange(false)
                      goTo(section.slug)
                    }}
                  >
                    <span>{t(section.title)}</span>
                    {!section.built ? (
                      <span className="ml-auto text-xs text-muted-foreground">
                        soon
                      </span>
                    ) : null}
                  </CommandItem>
                ))}
            </CommandGroup>
          ))}
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
