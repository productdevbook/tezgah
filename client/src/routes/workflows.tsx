import { createFileRoute, Outlet } from "@tanstack/react-router"

import { WorkflowsLayout } from "@/features/workflows/layout"

/** The layout every `/workflows/*` tab renders inside — see `routes/store.tsx`. */
export const Route = createFileRoute("/workflows")({
  component: RouteComponent,
})

export function RouteComponent() {
  return (
    <WorkflowsLayout>
      <Outlet />
    </WorkflowsLayout>
  )
}
