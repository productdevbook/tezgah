import { createFileRoute } from "@tanstack/react-router"

import { WorkflowDetail } from "@/features/workflows/detail"

export const Route = createFileRoute("/workflows_/$id")({
  component: RouteComponent,
})

export function RouteComponent() {
  const { id } = Route.useParams()
  return <WorkflowDetail id={id} />
}
