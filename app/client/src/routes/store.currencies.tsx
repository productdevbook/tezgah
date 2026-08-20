import { Outlet, createFileRoute } from "@tanstack/react-router"

import { StoreCurrencies } from "@/features/store/currencies"

export const Route = createFileRoute("/store/currencies")({
  component: RouteComponent,
})

/**
 * The `<Outlet />` is what this tab's `new` route draws into — a creation
 * form is a focus modal over the list, not a page that replaces it.
 */
export function RouteComponent() {
  return (
    <>
      <StoreCurrencies />
      <Outlet />
    </>
  )
}
