import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router"

import { AppShell } from "@/components/app-shell"
import { Overview } from "@/screens/overview"
import { SectionScreen } from "@/screens/section"

const rootRoute = createRootRoute({ component: AppShell })

const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: Overview,
})

const sectionRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/$section",
  component: function Section() {
    return <SectionScreen slug={sectionRoute.useParams().section} />
  },
})

const routeTree = rootRoute.addChildren([overviewRoute, sectionRoute])

export const router = createRouter({ routeTree })

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
