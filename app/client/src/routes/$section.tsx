import { createFileRoute, Navigate } from "@tanstack/react-router"

import { NotBuilt } from "@/features/not-built/screen"
import { sectionBySlug } from "@/lib/nav"
import { useHostScreen } from "@/panel/extensions"

/**
 * Every section without a screen of its own, behind one route.
 *
 * The seven built sections each have a real file in this directory and take
 * this one's place in the match — a static route always wins over a
 * dynamic one. What lands here is a slug this panel does not draw yet, or
 * one that names no section at all, and the two are told apart the same way
 * `lib/nav.ts` already did: a slug with no `Section` behind it goes home.
 */
export const Route = createFileRoute("/$section")({ component: SectionRoute })

export function SectionRoute() {
  const { section: slug } = Route.useParams()
  const host = useHostScreen(slug)
  if (host) return <>{host.render()}</>
  const section = sectionBySlug(slug)
  if (!section) return <Navigate to="/" replace />
  return <NotBuilt section={section} />
}
