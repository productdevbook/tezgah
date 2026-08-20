import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useState } from "react"
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
  patchOperator,
  ROLE_MEANS,
  whoAmI,
  type Operator,
  type Role,
} from "@/features/operators/api"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip"

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
                  <TableHead>Role</TableHead>
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
                    <TableCell>
                      <RolePicker row={row} isSelf={me.data?.id === row.id} />
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

/**
 * The role is enforced at the server's door, against the `Action` tezgah's own
 * route table declares — so this only has to say which of the three somebody
 * is, and what that means.
 *
 * The server refuses the last owner being narrowed, and refuses anybody but an
 * owner changing a role at all. Both come back as an error rather than being
 * hidden here: a shop with one owner should be able to read why, not find the
 * control missing.
 */
function RolePicker({ row, isSelf }: { row: Operator; isSelf: boolean }) {
  const client = useQueryClient()
  const [refused, setRefused] = useState<string | null>(null)

  const mutation = useMutation({
    mutationFn: (role: Role) => patchOperator(row.id, { role }),
    onSuccess: () => {
      setRefused(null)
      void client.invalidateQueries({ queryKey: ["operators"] })
    },
    onError: (error) =>
      setRefused(error instanceof Error ? error.message : "Refused."),
  })

  return (
    <div className="flex flex-col gap-1">
      <Tooltip>
        <TooltipTrigger
          render={
            <Select
              value={row.role}
              onValueChange={(value) => mutation.mutate(value as Role)}
              disabled={mutation.isPending}
            >
              <SelectTrigger size="sm" className="w-28">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {(Object.keys(ROLE_MEANS) as Role[]).map((option) => (
                  <SelectItem key={option} value={option}>
                    {option}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          }
        />
        <TooltipContent>{ROLE_MEANS[row.role]}</TooltipContent>
      </Tooltip>
      {isSelf ? (
        <span className="text-xs text-muted-foreground">you</span>
      ) : null}
      {refused ? (
        <span className="text-xs text-destructive">{refused}</span>
      ) : null}
    </div>
  )
}

function Toggle({ row, isSelf }: { row: Operator; isSelf: boolean }) {
  const client = useQueryClient()
  const mutation = useMutation({
    mutationFn: (disabled: boolean) => patchOperator(row.id, { disabled }),
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
