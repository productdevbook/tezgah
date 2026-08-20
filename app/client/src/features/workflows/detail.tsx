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
import { DetailField, FieldGrid, Empty } from "@/components/detail-fields"
import { DetailHeader } from "@/components/detail-header"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Card, CardContent } from "@/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime, useDetail } from "@/lib/detail"

function runTone(state: WorkflowRunState): "default" | "secondary" | "outline" | "destructive" {
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
  if (state === "invoking" || state === "called" || state === "compensating") return "secondary"
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
  const run = useDetail(["workflow-runs"], "/admin/workflows-executions/{id}", workflowRun, id)
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
    <div className="max-w-3xl space-y-4">
      <QueryState query={run} empty={{ title: "No run", description: "Nothing to show." }}>
        {(item) => (
          <>
            <DetailHeader back="workflows" title={`Run ${item.id}`}>
              <Badge variant={runTone(item.state)}>{item.state}</Badge>
            </DetailHeader>
            <Card>
              <CardContent>
                <FieldGrid>
                  <DetailField label="ID">
                    <span className="font-mono text-xs">{item.id}</span>
                  </DetailField>
                  <DetailField label="State">{item.state}</DetailField>
                  <DetailField label="Failure" full>
                    {item.failure ?? <Empty />}
                  </DetailField>
                </FieldGrid>
              </CardContent>
            </Card>
            <Card>
              <CardContent>
                <h2 className="mb-3 text-sm font-medium">Steps</h2>
                <StepsTable steps={steps} />
              </CardContent>
            </Card>
          </>
        )}
      </QueryState>
    </div>
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
              .sort((a, b) => a.group_ordering - b.group_ordering || a.ordering - b.ordering)
              .map((step) => (
                <TableRow key={step.id}>
                  <TableCell>
                    <div className="font-medium">{step.name}</div>
                    {step.failure ? (
                      <div className="text-destructive mt-0.5 max-w-sm truncate text-xs">
                        {step.failure}
                      </div>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    <Badge variant={stepTone(step.state)}>{step.state}</Badge>
                    <div className="text-muted-foreground mt-0.5 text-xs">
                      {STEP_MEANING[step.state]}
                    </div>
                  </TableCell>
                  <TableCell className="text-right font-mono text-xs">
                    {step.attempts}/{step.max_attempts}
                  </TableCell>
                  <TableCell className="text-muted-foreground text-xs">
                    {dateTime(step.run_after)}
                  </TableCell>
                  <TableCell className="text-muted-foreground text-xs">
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
