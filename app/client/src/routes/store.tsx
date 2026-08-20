import { createFileRoute, Outlet } from "@tanstack/react-router"

import { StoreLayout } from "@/features/store/layout"

/**
 * The layout every `/store/*` tab renders inside — the tabs and heading are
 * shared chrome, the same way `routes/__root.tsx` wraps every route in
 * `AppShell`. Which tab is open is `<Outlet />`'s business, not this file's.
 */
export const Route = createFileRoute("/store")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <StoreLayout>
      <Outlet />
    </StoreLayout>
  )
}
