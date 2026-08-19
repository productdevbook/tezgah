import { Link, Outlet, useMatchRoute } from "@tanstack/react-router"

import { Badge } from "@/components/ui/badge"
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
import { COVERAGE, GROUPS } from "@/lib/nav"

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
                  {group.sections.map((section) => {
                    const active = Boolean(
                      matchRoute({ to: "/$section", params: { section: section.slug }, fuzzy: true }),
                    )
                    return (
                      <SidebarMenuItem key={section.slug}>
                        <SidebarMenuButton
                          asChild
                          isActive={active}
                          tooltip={section.title}
                        >
                          <Link to="/$section" params={{ section: section.slug }}>
                            <span className="truncate">{section.title}</span>
                            {!section.built ? (
                              <span className="text-muted-foreground ml-auto text-[10px] group-data-[collapsible=icon]:hidden">
                                soon
                              </span>
                            ) : null}
                          </Link>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    )
                  })}
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
          <Badge variant="outline" className="ml-auto text-xs">
            no host connected
          </Badge>
        </header>
        <main className="flex-1 p-4 md:p-6">
          <Outlet />
        </main>
      </SidebarInset>
    </SidebarProvider>
  )
}
