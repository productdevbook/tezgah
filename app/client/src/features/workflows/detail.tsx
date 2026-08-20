import { useQuery, type UseQueryResult } from "@tanstack/react-query"
import { z } from "zod"

import { get } from "@/api/client"
import {
  workflowRun,
  workflowStep,
  type WorkflowRunState,
  type WorkflowStep,
  type WorkflowStepState,
} from "@/api/schemas"
import { Empty, Mono } from "@/components/detail-fields"
import { DetailPage } from "@/components/detail-page"
import { QueryState } from "@/components/query-state"
import { Section, SectionRow, SectionRows } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime, useDetail } from "@/lib/detail"

function runTone(
  state: WorkflowRunState
): "default" | "secondary" | "outline" | "destructive" {
  if (state === "done") return "default"
  if (state === "failed") return "destructive"
  if (state === "running" || state === "compensating") return "secondary"
  return "outline"
}

/**
 * A step's own nine states, told apart rather than painted one colour:
 * `skipped` reads as neutral, not a failure — it is `Outcome::skipped`, a
 * step with nothing to do — and `compensating`/`reverted` read as the trace
 * of an unwind rather than as `failed` itself.
 */
function stepTone(
  state: WorkflowStepState
): "default" | "secondary" | "outline" | "destructive" {
  if (state === "done") return "default"
  if (state === "failed") return "destructive"
  if (state === "invoking" || state === "called" || state === "compensating")
    return "secondary"
  return "outline"
}

const STEP_MEANING: Record<WorkflowStepState, string> = {
  pending: "not started yet",
  invoking: "running its do() now",
  called: "prepared and waiting to be resumed",
  waiting: "paused for something outside the run",
  done: "finished and wrote what it does",
  skipped: "had nothing to do — not an error, and nothing to undo",
  compensating: "undoing what it wrote, because a later step failed",
  reverted: "undone",
  failed: "ran out of retries",
}

export function WorkflowDetail({ id }: { id: string }) {
  const run = useDetail(
    ["workflow-runs"],
    "/admin/workflows-executions/{id}",
    workflowRun,
    id
  )
  const steps = useQuery({
    queryKey: ["workflow-runs", id, "steps"],
    queryFn: ({ signal }) =>
      get("/admin/workflows-executions/{id}/steps", {
        signal,
        schema: z.array(workflowStep),
        params: { id },
      }),
  })

  return (
    <DetailPage
      query={run}
      empty={{ title: "No run", description: "Nothing to show." }}
      back="workflows"
      title={(item) => `Run ${item.id}`}
      actions={(item) => (
        <Badge variant={runTone(item.state)}>{item.state}</Badge>
      )}
      main={() => (
        <Section
          title="Steps"
          description="Each declares how to undo itself, so a failure late in the run walks back through everything before it."
        >
          <StepsTable steps={steps} />
        </Section>
      )}
      side={(item) => (
        <>
          <Section title="The run">
            <SectionRows>
              <SectionRow label="State" value={item.state} />
              <SectionRow label="Failure" value={item.failure} />
            </SectionRows>
          </Section>

          <Section title="Details">
            <SectionRows>
              <SectionRow label="ID" value={<Mono>{item.id}</Mono>} />
            </SectionRows>
          </Section>
        </>
      )}
    />
  )
}

function StepsTable({ steps }: { steps: UseQueryResult<WorkflowStep[]> }) {
  return (
    <QueryState
      query={steps}
      empty={{ title: "No steps", description: "This workflow declared none." }}
    >
      {(items) => (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Step</TableHead>
              <TableHead>State</TableHead>
              <TableHead className="text-right">Attempts</TableHead>
              <TableHead>Run after</TableHead>
              <TableHead>Lease until</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {[...items]
              .sort(
                (a, b) =>
                  a.group_ordering - b.group_ordering || a.ordering - b.ordering
              )
              .map((step) => (
                <TableRow key={step.id}>
                  <TableCell>
                    <div className="font-medium">{step.name}</div>
                    {step.failure ? (
                      <div className="mt-0.5 max-w-sm truncate text-xs text-destructive">
                        {step.failure}
                      </div>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    <Badge variant={stepTone(step.state)}>{step.state}</Badge>
                    <div className="mt-0.5 text-xs text-muted-foreground">
                      {STEP_MEANING[step.state]}
                    </div>
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs">
                    {step.attempts}/{step.max_attempts}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {dateTime(step.run_after)}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {step.lease_until ? dateTime(step.lease_until) : <Empty />}
                  </TableCell>
                </TableRow>
              ))}
          </TableBody>
        </Table>
      )}
    </QueryState>
  )
}
