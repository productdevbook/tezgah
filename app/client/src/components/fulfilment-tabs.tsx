import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "providers", label: "Providers", to: "/fulfilment/providers" },
  { value: "sets", label: "Fulfilment sets", to: "/fulfilment/sets" },
  {
    value: "shipping-options",
    label: "Shipping options",
    to: "/fulfilment/shipping-options",
  },
  {
    value: "shipping-option-types",
    label: "Shipping option types",
    to: "/fulfilment/shipping-option-types",
  },
  {
    value: "shipping-profiles",
    label: "Shipping profiles",
    to: "/fulfilment/shipping-profiles",
  },
] as const

/** Same shape as `components/store-tabs.tsx`: each tab is a real route. */
export function FulfilmentTabs() {
  const matchRoute = useMatchRoute()
  const active = TABS.find((tab) =>
    matchRoute({ to: tab.to, fuzzy: true })
  )?.value

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
