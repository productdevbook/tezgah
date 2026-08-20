import { z } from "zod"

import { parseResponse } from "@/api/drift"
import { apiMutator } from "@/api/mutator"

/**
 * The server's own two records, transcribed from `http::auth`'s views.
 *
 * Not tezgah's: the crate asks a host to keep an audit trail and an outbox
 * through `ports::AuditSink` and `ports::EventSink`, and says nothing about
 * how either is read back. This is how this host reads them.
 */
export const auditRow = z.object({
  id: z.string(),
  actor_kind: z.string(),
  actor_id: z.string().nullable(),
  action: z.string(),
  entity: z.string(),
  entity_id: z.string(),
  summary: z.unknown(),
  created_at: z.string(),
})

export type AuditRow = z.infer<typeof auditRow>

export const eventRow = z.object({
  id: z.string(),
  name: z.string(),
  entity_id: z.string(),
  payload: z.unknown(),
  created_at: z.string(),
  delivered_at: z.string().nullable(),
})

export type EventRow = z.infer<typeof eventRow>

async function recent<S extends z.ZodTypeAny>(
  path: string,
  schema: S,
  signal?: AbortSignal
): Promise<z.infer<S>[]> {
  const { data, status } = await apiMutator<{ data: unknown; status: number }>(
    path,
    { method: "GET", signal }
  )
  return parseResponse(z.array(schema), data, status)
}

export const listAudit = (signal?: AbortSignal) =>
  recent("/admin/records/audit", auditRow, signal)

export const listEvents = (signal?: AbortSignal) =>
  recent("/admin/records/events", eventRow, signal)
