import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "payments", label: "Payments", to: "/payments" },
  { value: "refund-reasons", label: "Refund reasons", to: "/payments/refund-reasons" },
] as const

/** Same shape as `components/payouts-tabs.tsx`: each tab is a real route. */
export function PaymentsTabs() {
  const matchRoute = useMatchRoute()
  const active = TABS.find((tab) =>
    matchRoute({ to: tab.to, fuzzy: tab.value !== "payments" })
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
