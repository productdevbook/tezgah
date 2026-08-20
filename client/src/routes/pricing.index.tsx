import { createFileRoute, redirect } from "@tanstack/react-router"

/** `/pricing` names no tab of its own — send it to the first one. */
export const Route = createFileRoute("/pricing/")({
  beforeLoad: () => {
    throw redirect({ to: "/pricing/price-lists" })
  },
})
