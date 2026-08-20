import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "rates", label: "Tax rates", to: "/tax/rates" },
  { value: "regions", label: "Tax regions", to: "/tax/regions" },
  { value: "registrations", label: "Registrations", to: "/tax/registrations" },
] as const

/** Same shape as `components/store-tabs.tsx`: each tab is a real route. */
export function TaxTabs() {
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
