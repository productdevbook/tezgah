import { createFileRoute, redirect } from "@tanstack/react-router"

/** `/store` names no tab of its own — send it to the first one. */
export const Route = createFileRoute("/store/")({
  beforeLoad: () => {
    throw redirect({ to: "/store/currencies" })
  },
})
