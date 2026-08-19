import {
  createRootRoute,
  createRoute,
  createRouter,
  Navigate,
} from "@tanstack/react-router"

import { AppShell } from "@/components/app-shell"
import { sectionBySlug } from "@/lib/nav"
import { Inventory } from "@/screens/inventory"
import { NotBuilt } from "@/screens/not-built"
import { Orders } from "@/screens/orders"
import { Overview } from "@/screens/overview"
import { Products } from "@/screens/products"

const rootRoute = createRootRoute({ component: AppShell })

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Overview,
})

/**
 * One route over every section, rather than one route each.
 *
 * The sections come from the same table the sidebar reads, so a section that
 * exists in navigation and nowhere in routing cannot happen — the failure this
 * shape removes is a menu entry that leads to a blank page.
 */
const sectionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/$section",
  component: SectionScreen,
})

const BUILT: Record<string, () => React.ReactElement> = {
  products: Products,
  orders: Orders,
  inventory: Inventory,
}

function SectionScreen() {
  const { section: slug } = sectionRoute.useParams()
  const section = sectionBySlug(slug)

  if (!section) return <Navigate to="/" replace />

  const Screen = BUILT[section.slug]
  return Screen ? <Screen /> : <NotBuilt section={section} />
}

const routeTree = rootRoute.addChildren([overviewRoute, sectionRoute])

export const router = createRouter({ routeTree })

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
