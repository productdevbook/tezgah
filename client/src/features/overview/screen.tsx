import { useQuery } from "@tanstack/react-query"

import { z } from "zod"

import { ApiError, get } from "@/api/client"
import { Badge } from "@/components/ui/badge"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"
import { COVERAGE, GROUPS } from "@/lib/nav"
import { PageHeading } from "@/components/page-heading"

/**
 * What this panel can and cannot do, said before anything else.
 *
 * A dashboard of counts would be the ordinary thing here and would be made up:
 * tezgah serves nothing itself, so there is no shop to count. What is true and
 * worth showing is how much of the declared surface has a screen.
 */
export function Overview() {
  const host = useQuery({
    queryKey: ["host"],
    // Only whether anybody answers; the shape is deliberately not asserted.
    queryFn: ({ signal }) =>
      get("/admin/currencies", { signal, schema: z.unknown(), query: { limit: 1 } }),
    retry: false,
  })

  const reachable = host.isSuccess
  const unreachable =
    host.isError && host.error instanceof ApiError && host.error.kind === "unreachable"

  const percent = Math.round((COVERAGE.covered / COVERAGE.operations) * 100)

  return (
    <div className="space-y-6">
      <PageHeading
        title="Overview"
        subtitle="tezgah's admin surface, and how much of it this panel covers."
      />

      <div className="grid gap-4 md:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-base">The host</CardTitle>
            <CardDescription>
              tezgah is a library. Something else has to mount `api::routes()`.
            </CardDescription>
          </CardHeader>
          <CardContent>
            {host.isPending ? (
              <Badge variant="outline">asking…</Badge>
            ) : reachable ? (
              <Badge>answering</Badge>
            ) : unreachable ? (
              <div className="space-y-1">
                <Badge variant="destructive">nobody answering</Badge>
                <p className="text-muted-foreground text-xs">
                  Set <code>VITE_TEZGAH_API</code> to where the API is served.
                </p>
              </div>
            ) : (
              <Badge variant="outline">answered, but refused</Badge>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-base">Coverage</CardTitle>
            <CardDescription>
              {COVERAGE.covered} of {COVERAGE.operations} declared admin
              operations sit behind a screen.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            <Progress value={percent} />
            <p className="text-muted-foreground text-xs">{percent}%</p>
          </CardContent>
        </Card>
      </div>

      <div className="space-y-4">
        {GROUPS.map((group) => (
          <div key={group.title} className="space-y-2">
            <h2 className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              {group.title}
            </h2>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
              {group.sections.filter((s) => !s.folded).map((section) => (
                <div
                  key={section.slug}
                  className="flex items-center justify-between gap-2 rounded-md border px-3 py-2"
                >
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">
                      {section.title}
                    </div>
                    <div className="text-muted-foreground text-xs">
                      {section.operations} operations
                    </div>
                  </div>
                  {section.built ? (
                    <Badge>built</Badge>
                  ) : (
                    <Badge variant="outline">soon</Badge>
                  )}
                </div>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
