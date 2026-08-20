import { createFileRoute, redirect } from "@tanstack/react-router"

/** `/fulfilment` names no tab of its own — send it to the first one. */
export const Route = createFileRoute("/fulfilment/")({
  beforeLoad: () => {
    throw redirect({ to: "/fulfilment/providers" })
  },
})
