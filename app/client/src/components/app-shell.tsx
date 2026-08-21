import { Link, Outlet, useMatchRoute } from "@tanstack/react-router"

import { Button } from "@/components/ui/button"
import { Separator } from "@/components/ui/separator"
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarRail,
  SidebarTrigger,
} from "@/components/ui/sidebar"
import { CommandPalette } from "@/components/command-palette"
import { SectionLink } from "@/components/section-link"
import { useCommandPalette } from "@/lib/command-palette"
import { COVERAGE, GROUPS, SERVER_SECTIONS, type Section } from "@/lib/nav"
import { mayOpen, useWhoAmI } from "@/lib/session"
import { useT } from "@/panel/i18n"
import { panelRuntime } from "@/panel/runtime"

/// The host's own screens are not in `routes()` and not in `GROUPS`, so they
/// get their own answer to the same question.
function isActiveServerSection(
  matchRoute: ReturnType<typeof useMatchRoute>,
  slug: string
): boolean {
  switch (slug) {
    case "operators":
      return Boolean(matchRoute({ to: "/operators", fuzzy: true }))
    case "batch":
      return Boolean(matchRoute({ to: "/batch", fuzzy: true }))
    case "records":
      return Boolean(matchRoute({ to: "/records", fuzzy: true }))
    default:
      return false
  }
}

function isActiveSection(
  matchRoute: ReturnType<typeof useMatchRoute>,
  section: Section
): boolean {
  switch (section.slug) {
    case "products":
      return Boolean(matchRoute({ to: "/products", fuzzy: true }))
    case "orders":
      return Boolean(matchRoute({ to: "/orders", fuzzy: true }))
    case "inventory":
      return Boolean(matchRoute({ to: "/inventory", fuzzy: true }))
    case "customers":
      return Boolean(matchRoute({ to: "/customers", fuzzy: true }))
    case "promotions":
      return Boolean(matchRoute({ to: "/promotions", fuzzy: true }))
    case "subscriptions":
      return Boolean(matchRoute({ to: "/subscriptions", fuzzy: true }))
    case "store":
      return Boolean(matchRoute({ to: "/store", fuzzy: true }))
    case "payouts":
      return Boolean(matchRoute({ to: "/payouts", fuzzy: true }))
    case "workflows":
      return Boolean(matchRoute({ to: "/workflows", fuzzy: true }))
    case "baskets":
      return Boolean(matchRoute({ to: "/baskets", fuzzy: true }))
    case "fulfilment":
      return Boolean(matchRoute({ to: "/fulfilment", fuzzy: true }))
    case "tax":
      return Boolean(matchRoute({ to: "/tax", fuzzy: true }))
    case "pricing":
      return Boolean(matchRoute({ to: "/pricing", fuzzy: true }))
    case "payments":
      return Boolean(matchRoute({ to: "/payments", fuzzy: true }))
    case "credit":
      return Boolean(matchRoute({ to: "/credit", fuzzy: true }))
    case "carts":
      return Boolean(matchRoute({ to: "/carts", fuzzy: true }))
    case "operators":
      return Boolean(matchRoute({ to: "/operators", fuzzy: true }))
    default:
      return Boolean(
        matchRoute({
          to: "/$section",
          params: { section: section.slug },
          fuzzy: true,
        })
      )
  }
}

export function AppShell() {
  const matchRoute = useMatchRoute()
  const palette = useCommandPalette()
  const me = useWhoAmI()
  const t = useT()

  return (
    <SidebarProvider>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <div className="flex items-center gap-2 px-2 py-1.5">
            <div className="flex size-7 shrink-0 items-center justify-center rounded-md bg-primary text-xs font-semibold text-primary-foreground">
              tz
            </div>
            <div className="grid min-w-0 leading-tight group-data-[collapsible=icon]:hidden">
              <span className="truncate text-sm font-medium">tezgah</span>
              <span className="truncate text-xs text-muted-foreground">
                admin
              </span>
            </div>
          </div>
        </SidebarHeader>

        <SidebarContent>
          {GROUPS.map((group) => (
            <SidebarGroup key={group.title}>
              <SidebarGroupLabel>{t(group.title)}</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {group.sections
                    .filter((section) => !section.folded)
                    .map((section) => (
                      <SidebarMenuItem key={section.slug}>
                        <SidebarMenuButton
                          isActive={isActiveSection(matchRoute, section)}
                          tooltip={t(section.title)}
                          render={<SectionLink slug={section.slug} />}
                        >
                          <span className="truncate">{t(section.title)}</span>
                          {!section.built ? (
                            <span className="ml-auto text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden">
                              {t("nav.soon")}
                            </span>
                          ) : null}
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
          <SidebarGroup>
            <SidebarGroupLabel>{t("nav.group.thisServer")}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {SERVER_SECTIONS.filter((section) =>
                  mayOpen(section.slug, me.data)
                ).map((section) => (
                  <SidebarMenuItem key={section.slug}>
                    <SidebarMenuButton
                      isActive={isActiveServerSection(matchRoute, section.slug)}
                      tooltip={t(section.title)}
                      render={<SectionLink slug={section.slug} />}
                    >
                      <span className="truncate">{t(section.title)}</span>
                    </SidebarMenuButton>
                  </SidebarMenuItem>
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        </SidebarContent>

        <SidebarFooter>
          <div className="grid gap-1 px-2 py-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
            <span className="truncate">
              {me.data === null
                ? t("nav.adminToken")
                : me.data
                  ? `${me.data.name} · ${me.data.role}`
                  : ""}
            </span>
            <span>
              {t("nav.coverage", {
                covered: COVERAGE.covered,
                operations: COVERAGE.operations,
              })}
            </span>
          </div>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>

      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <Link to="/" className="text-sm font-medium">
            {t("nav.overview")}
          </Link>
          <Button
            variant="outline"
            size="sm"
            className="ml-auto gap-2 font-normal text-muted-foreground"
            onClick={() => palette.setOpen(true)}
          >
            {t("nav.goTo")}
            <kbd className="rounded bg-muted px-1 text-[10px]">⌘K</kbd>
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="text-xs"
            onClick={() => panelRuntime().onUnauthenticated()}
          >
            {t("nav.disconnect")}
          </Button>
        </header>
        <main className="flex-1 p-4 md:p-6">
          <Outlet />
        </main>
        <CommandPalette open={palette.open} onOpenChange={palette.setOpen} />
      </SidebarInset>
    </SidebarProvider>
  )
}
