import { useQuery } from "@tanstack/react-query"

import { TableFrame } from "@/components/data-table"
import { Mono } from "@/components/detail-fields"
import { PageHeading } from "@/components/page-heading"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { dateTime } from "@/lib/detail"
import {
  listAudit,
  listEvents,
  type AuditRow,
  type EventRow,
} from "@/features/records/api"

/**
 * What the shop wrote down, and what it has to say.
 *
 * Both are written in the transaction of the change they belong to, which is
 * the property that makes them worth reading: a row here is a thing that
 * happened, and a change that rolled back left none.
 *
 * Newest first and a fixed ceiling rather than a cursor: what this answers is
 * "what just happened". A longer question wants the database.
 */
export function Records() {
  return (
    <div className="space-y-4">
      <PageHeading
        title="What happened"
        subtitle="The audit trail and the outbox, newest first."
      />
      <Audit />
      <Events />
    </div>
  )
}

function Audit() {
  const query = useQuery({
    queryKey: ["records", "audit"],
    queryFn: ({ signal }) => listAudit(signal),
  })

  return (
    <QueryState
      query={query}
      empty={{
        title: "Nothing written down yet",
        description: "An audit row is written when something changes.",
      }}
    >
      {(rows: AuditRow[]) => (
        <TableFrame
          header={{
            title: "Audit",
            description:
              "Who did what to which row. An ADMIN_TOKEN request names nobody, because a shared secret is not a person.",
          }}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>When</TableHead>
                <TableHead>Who</TableHead>
                <TableHead>Did</TableHead>
                <TableHead>To</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="text-xs text-muted-foreground">
                    {dateTime(row.created_at)}
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-1.5">
                      <Badge variant="outline">{row.actor_kind}</Badge>
                      {row.actor_id &&
                      row.actor_id !==
                        "00000000-0000-0000-0000-000000000000" ? (
                        <Mono>{row.actor_id.slice(0, 8)}</Mono>
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell>{row.action}</TableCell>
                  <TableCell>
                    {row.entity} <Mono>{row.entity_id.slice(0, 8)}</Mono>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
    </QueryState>
  )
}

function Events() {
  const query = useQuery({
    queryKey: ["records", "events"],
    queryFn: ({ signal }) => listEvents(signal),
  })

  return (
    <QueryState
      query={query}
      empty={{
        title: "Nothing to say yet",
        description:
          "An event is written when something worth telling happens.",
      }}
    >
      {(rows: EventRow[]) => (
        <TableFrame
          header={{
            title: "Outbox",
            description:
              "What the shop has to say. Nothing here delivers them — this server has no mailer and no HTTP client, so they are read rather than pushed.",
          }}
        >
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>When</TableHead>
                <TableHead>What</TableHead>
                <TableHead>About</TableHead>
                <TableHead className="text-right">Delivered</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {rows.map((row) => (
                <TableRow key={row.id}>
                  <TableCell className="text-xs text-muted-foreground">
                    {dateTime(row.created_at)}
                  </TableCell>
                  <TableCell>
                    <Mono>{row.name}</Mono>
                  </TableCell>
                  <TableCell>
                    <Mono>{row.entity_id.slice(0, 8)}</Mono>
                  </TableCell>
                  <TableCell className="text-right">
                    {row.delivered_at ? (
                      <span className="text-xs text-muted-foreground">
                        {dateTime(row.delivered_at)}
                      </span>
                    ) : (
                      <Badge variant="outline">waiting</Badge>
                    )}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </TableFrame>
      )}
    </QueryState>
  )
}
