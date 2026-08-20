import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { Add01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { Link } from "@tanstack/react-router"

import { TableFrame } from "@/components/data-table"
import { Mono } from "@/components/detail-fields"
import { PageHeading } from "@/components/page-heading"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
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
  listOperators,
  setDisabled,
  whoAmI,
  type Operator,
} from "@/features/operators/api"

/**
 * Who may reach this back office.
 *
 * Not one of tezgah's sections: the crate authenticates nobody and declares
 * no route for any of this. It is the server's own, and the sidebar says so
 * by keeping it out of the coverage tally rather than counting it as an
 * operation somebody drew.
 */
export function Operators() {
  const query = useQuery({
    queryKey: ["operators"],
    queryFn: ({ signal }) => listOperators(signal),
  })

  const me = useQuery({
    queryKey: ["whoami"],
    queryFn: ({ signal }) => whoAmI(signal),
  })

  return (
    <div className="space-y-4">
      <PageHeading
        title="Operators"
        subtitle="An account belongs to a person and can be revoked. The admin token belongs to nobody and cannot."
      >
        <Button
          size="sm"
          nativeButton={false}
          render={<Link to="/operators/new" />}
        >
          <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
          New operator
        </Button>
      </PageHeading>

      {me.data === null ? (
        <p className="text-sm text-muted-foreground">
          You are signed in with the admin token, so nothing you change here
          will be attributed to anybody. Make an account and use it.
        </p>
      ) : null}

      <QueryState
        query={query}
        empty={{
          title: "No accounts",
          description:
            "Only the admin token can reach this back office. Make an account.",
        }}
      >
        {(operators: Operator[]) => (
          <TableFrame
            header={{
              title: "Accounts",
              description:
                "Disabling one ends every session it holds, in the same transaction.",
            }}
          >
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead>E-mail</TableHead>
                  <TableHead>Since</TableHead>
                  <TableHead className="text-right">State</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {operators.map((row) => (
                  <TableRow key={row.id}>
                    <TableCell className="font-medium">{row.name}</TableCell>
                    <TableCell>
                      <Mono>{row.email}</Mono>
                    </TableCell>
                    <TableCell className="text-xs text-muted-foreground">
                      {dateTime(row.created_at)}
                    </TableCell>
                    <TableCell className="text-right">
                      <Toggle row={row} isSelf={me.data?.id === row.id} />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </TableFrame>
        )}
      </QueryState>
    </div>
  )
}

function Toggle({ row, isSelf }: { row: Operator; isSelf: boolean }) {
  const client = useQueryClient()
  const mutation = useMutation({
    mutationFn: (disabled: boolean) => setDisabled(row.id, disabled),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["operators"] })
    },
  })

  const disabled = row.disabled_at !== null

  return (
    <div className="flex items-center justify-end gap-2">
      <Badge variant={disabled ? "outline" : "default"}>
        {disabled ? "disabled" : "active"}
      </Badge>
      <Button
        size="sm"
        variant="outline"
        // The server refuses this too. Both, because a button that looks
        // available and answers with an error is worse than one that says no.
        disabled={mutation.isPending || (isSelf && !disabled)}
        onClick={() => mutation.mutate(!disabled)}
      >
        {disabled ? "Enable" : "Disable"}
      </Button>
    </div>
  )
}
