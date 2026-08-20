import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "currencies", label: "Currencies", to: "/store/currencies" },
  { value: "regions", label: "Regions", to: "/store/regions" },
  { value: "sales-channels", label: "Sales channels", to: "/store/sales-channels" },
  { value: "keys", label: "Publishable keys", to: "/store/keys" },
] as const

/**
 * The store's tabs, as routes rather than client state — each one is a real
 * address (`/store/regions`, ...), found by matching the URL the same way
 * `components/app-shell.tsx` picks the active section, not by owning a
 * `value`/`onValueChange` pair of its own.
 */
export function StoreTabs() {
  const matchRoute = useMatchRoute()
  const active = TABS.find((tab) => matchRoute({ to: tab.to, fuzzy: true }))?.value

  return (
    <Tabs value={active ?? null}>
      <TabsList>
        {TABS.map((tab) => (
          <TabsTrigger
            key={tab.value}
            value={tab.value}
            nativeButton={false}
            render={<Link to={tab.to} />}
          >
            {tab.label}
          </TabsTrigger>
        ))}
      </TabsList>
    </Tabs>
  )
}
