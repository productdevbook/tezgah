import { createFileRoute } from "@tanstack/react-router"
import { z } from "zod"

import { workflowRunState } from "@/api/schemas"
import { Workflows } from "@/features/workflows/screen"

const workflowsSearch = z.object({
  after: z.string().optional(),
  state: workflowRunState.optional(),
})

export const Route = createFileRoute("/workflows/")({
  validateSearch: workflowsSearch,
  component: RouteComponent,
})

export function RouteComponent() {
  const { state, after } = Route.useSearch()
  const navigate = Route.useNavigate()

  return (
    <Workflows
      state={state ?? "all"}
      after={after}
      onStateChange={(next) =>
        void navigate({
          search: (prev) => ({
            ...prev,
            state: next === "all" ? undefined : next,
            after: undefined,
          }),
        })
      }
      onAfterChange={(next) =>
        void navigate({ search: (prev) => ({ ...prev, after: next }) })
      }
    />
  )
}
