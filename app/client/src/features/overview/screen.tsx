import { useQuery } from "@tanstack/react-query"

import { z } from "zod"

import { ApiError, get } from "@/api/client"
import { Section, SectionBody } from "@/components/section"
import { SectionLink } from "@/components/section-link"
import { Badge } from "@/components/ui/badge"
import { Progress } from "@/components/ui/progress"
import { COVERAGE, GROUPS } from "@/lib/nav"
import { PageHeading } from "@/components/page-heading"
import { Zone } from "@/panel/zone"
import { useT } from "@/panel/i18n"

/**
 * What this panel can and cannot do, said before anything else.
 *
 * A dashboard of counts would be the ordinary thing here and would be made up:
 * tezgah serves nothing itself, so there is no shop to count. What is true and
 * worth showing is how much of the declared surface has a screen.
 */
export function Overview() {
  const t = useT()
  const host = useQuery({
    queryKey: ["host"],
    // Only whether anybody answers; the shape is deliberately not asserted.
    queryFn: ({ signal }) =>
      get("/admin/currencies", {
        signal,
        schema: z.unknown(),
        query: { limit: 1 },
      }),
    retry: false,
  })

  const reachable = host.isSuccess
  const unreachable =
    host.isError &&
    host.error instanceof ApiError &&
    host.error.kind === "unreachable"

  const percent = Math.round((COVERAGE.covered / COVERAGE.operations) * 100)

  return (
    <div className="space-y-6">
      <PageHeading title={t("overview.title")} subtitle={t("overview.why")} />

      <div className="grid gap-4 md:grid-cols-2">
        <Section title={t("overview.host")} description={t("overview.hostWhy")}>
          <SectionBody>
            {host.isPending ? (
              <Badge variant="outline">asking…</Badge>
            ) : reachable ? (
              <Badge>answering</Badge>
            ) : unreachable ? (
              <div className="space-y-1">
                <Badge variant="destructive">nobody answering</Badge>
                <p className="text-xs text-muted-foreground">
                  Set <code>VITE_TEZGAH_API</code> to where the API is served.
                </p>
              </div>
            ) : (
              <Badge variant="outline">answered, but refused</Badge>
            )}
          </SectionBody>
        </Section>

        <Section
          title={t("overview.coverage")}
          description={`${COVERAGE.covered} of ${COVERAGE.operations} declared admin operations sit behind a screen.`}
        >
          <SectionBody>
            <div className="space-y-2">
              <Progress value={percent} />
              <p className="text-xs text-muted-foreground">{percent}%</p>
            </div>
          </SectionBody>
        </Section>
      </div>

      <div className="space-y-4">
        {GROUPS.map((group) => (
          <div key={group.title} className="space-y-2">
            <h2 className="text-xs font-medium tracking-wide text-muted-foreground uppercase">
              {t(group.title)}
            </h2>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
              {group.sections
                .filter((s) => !s.folded)
                .map((section) => (
                  // A tile that names a section and does not go there is a
                  // dead end on the first screen somebody sees.
                  <SectionLink
                    key={section.slug}
                    slug={section.slug}
                    className="flex items-center justify-between gap-2 rounded-xl border bg-card px-3 py-2 transition-colors hover:bg-accent"
                  >
                    <span className="min-w-0">
                      <span className="block truncate text-sm font-medium">
                        {t(section.title)}
                      </span>
                      <span className="block text-xs text-muted-foreground">
                        {t("nav.operations", { n: section.operations })}
                      </span>
                    </span>
                    {section.built ? (
                      <Badge>{t("nav.built")}</Badge>
                    ) : (
                      <Badge variant="outline">{t("nav.soon")}</Badge>
                    )}
                  </SectionLink>
                ))}
            </div>
          </div>
        ))}
      </div>

      <Zone name="dashboard" />
    </div>
  )
}
