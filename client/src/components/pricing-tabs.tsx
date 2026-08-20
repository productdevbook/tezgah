import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "price-lists", label: "Price lists", to: "/pricing/price-lists" },
  { value: "price-preferences", label: "Price preferences", to: "/pricing/price-preferences" },
  { value: "price-sets", label: "Price sets", to: "/pricing/price-sets" },
  { value: "prices", label: "Prices", to: "/pricing/prices" },
] as const

/** Same shape as `components/store-tabs.tsx`: each tab is a real route. */
export function PricingTabs() {
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
