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
import { COVERAGE, GROUPS, type Section } from "@/lib/nav"
import { forget } from "@/lib/token"

/**
 * The seven built sections each have their own top-level route (their slug
 * is that route's path, by construction); everything else falls through to
 * the single `/$section` catch-all. One switch, rather than typing `Link`'s
 * `to` from a runtime string, because the router's route union is closed and
 * a template string can't join it.
 */
function SectionLink({
  slug,
  title,
  active,
  children,
}: {
  slug: string
  title: string
  active: boolean
  children: React.ReactNode
}) {
  switch (slug) {
    case "products":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/products" />}>
          {children}
        </SidebarMenuButton>
      )
    case "orders":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/orders" />}>
          {children}
        </SidebarMenuButton>
      )
    case "inventory":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/inventory" />}>
          {children}
        </SidebarMenuButton>
      )
    case "customers":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/customers" />}>
          {children}
        </SidebarMenuButton>
      )
    case "promotions":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/promotions" />}>
          {children}
        </SidebarMenuButton>
      )
    case "subscriptions":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/subscriptions" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "store":
      return (
        <SidebarMenuButton isActive={active} tooltip={title} render={<Link to="/store" />}>
          {children}
        </SidebarMenuButton>
      )
    default:
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/$section" params={{ section: slug }} />}
        >
          {children}
        </SidebarMenuButton>
      )
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
    default:
      return Boolean(
        matchRoute({ to: "/$section", params: { section: section.slug }, fuzzy: true })
      )
  }
}

export function AppShell() {
  const matchRoute = useMatchRoute()

  return (
    <SidebarProvider>
      <Sidebar collapsible="icon">
        <SidebarHeader>
          <div className="flex items-center gap-2 px-2 py-1.5">
            <div className="bg-primary text-primary-foreground flex size-7 shrink-0 items-center justify-center rounded-md text-xs font-semibold">
              tz
            </div>
            <div className="grid min-w-0 leading-tight group-data-[collapsible=icon]:hidden">
              <span className="truncate text-sm font-medium">tezgah</span>
              <span className="text-muted-foreground truncate text-xs">
                admin
              </span>
            </div>
          </div>
        </SidebarHeader>

        <SidebarContent>
          {GROUPS.map((group) => (
            <SidebarGroup key={group.title}>
              <SidebarGroupLabel>{group.title}</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {group.sections.map((section) => (
                    <SidebarMenuItem key={section.slug}>
                      <SectionLink
                        slug={section.slug}
                        title={section.title}
                        active={isActiveSection(matchRoute, section)}
                      >
                        <span className="truncate">{section.title}</span>
                        {!section.built ? (
                          <span className="text-muted-foreground ml-auto text-[10px] group-data-[collapsible=icon]:hidden">
                            soon
                          </span>
                        ) : null}
                      </SectionLink>
                    </SidebarMenuItem>
                  ))}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          ))}
        </SidebarContent>

        <SidebarFooter>
          <div className="text-muted-foreground px-2 py-1 text-xs group-data-[collapsible=icon]:hidden">
            {COVERAGE.covered} of {COVERAGE.operations} admin operations have a
            screen
          </div>
        </SidebarFooter>
        <SidebarRail />
      </Sidebar>

      <SidebarInset>
        <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
          <SidebarTrigger />
          <Separator orientation="vertical" className="mr-1 h-4" />
          <Link to="/" className="text-sm font-medium">
            Overview
          </Link>
          <Button
            variant="ghost"
            size="sm"
            className="ml-auto text-xs"
            onClick={() => {
              forget()
              location.reload()
            }}
          >
            Disconnect
          </Button>
        </header>
        <main className="flex-1 p-4 md:p-6">
          <Outlet />
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
