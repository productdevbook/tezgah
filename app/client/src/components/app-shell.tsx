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
import { panelRuntime } from "@/panel/runtime"

/**
 * Every built section has its own top-level route (their slug is that
 * route's path, by construction); everything else falls through to the
 * single `/$section` catch-all. One switch, rather than typing `Link`'s `to`
 * from a runtime string, because the router's route union is closed and a
 * template string can't join it.
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
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/products" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "orders":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/orders" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "inventory":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/inventory" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "customers":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/customers" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "promotions":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/promotions" />}
        >
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
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/store" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "payouts":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/payouts" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "workflows":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/workflows" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "baskets":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/baskets" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "fulfilment":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/fulfilment" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "tax":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/tax" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "pricing":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/pricing" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "payments":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/payments" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "credit":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/credit" />}
        >
          {children}
        </SidebarMenuButton>
      )
    case "carts":
      return (
        <SidebarMenuButton
          isActive={active}
          tooltip={title}
          render={<Link to="/carts" />}
        >
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
              <SidebarGroupLabel>{group.title}</SidebarGroupLabel>
              <SidebarGroupContent>
                <SidebarMenu>
                  {group.sections
                    .filter((section) => !section.folded)
                    .map((section) => (
                      <SidebarMenuItem key={section.slug}>
                        <SectionLink
                          slug={section.slug}
                          title={section.title}
                          active={isActiveSection(matchRoute, section)}
                        >
                          <span className="truncate">{section.title}</span>
                          {!section.built ? (
                            <span className="ml-auto text-[10px] text-muted-foreground group-data-[collapsible=icon]:hidden">
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
          <div className="px-2 py-1 text-xs text-muted-foreground group-data-[collapsible=icon]:hidden">
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
            onClick={() => panelRuntime().onUnauthenticated()}
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
