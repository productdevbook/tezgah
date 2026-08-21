import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "payouts", label: "Payouts", to: "/payouts" },
  {
    value: "commission-rules",
    label: "Commission rules",
    to: "/payouts/commission-rules",
  },
] as const

/** Same shape as `components/store-tabs.tsx`: each tab is a real route. */
export function PayoutsTabs() {
  const matchRoute = useMatchRoute()
  const active = TABS.find((tab) =>
    matchRoute({ to: tab.to, fuzzy: tab.value !== "payouts" })
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
