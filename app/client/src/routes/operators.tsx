import { Outlet, createFileRoute } from "@tanstack/react-router"

import { Operators } from "@/features/operators/screen"

export const Route = createFileRoute("/operators")({
  component: RouteComponent,
})

/** The `<Outlet />` is what `/operators/new` draws into. */
export function RouteComponent() {
  return (
    <>
      <Operators />
      <Outlet />
    </>
  )
}
