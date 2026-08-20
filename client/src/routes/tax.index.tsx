import { createFileRoute, redirect } from "@tanstack/react-router"

/** `/tax` names no tab of its own — send it to the first one. */
export const Route = createFileRoute("/tax/")({
  beforeLoad: () => {
    throw redirect({ to: "/tax/rates" })
  },
})
