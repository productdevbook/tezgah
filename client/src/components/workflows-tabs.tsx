import { Link, useMatchRoute } from "@tanstack/react-router"

import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs"

const TABS = [
  { value: "executions", label: "Executions", to: "/workflows" },
  { value: "dead-letters", label: "Dead letters", to: "/workflows/dead-letters" },
] as const

/** Same shape as `components/store-tabs.tsx`: each tab is a real route. */
export function WorkflowsTabs() {
  const matchRoute = useMatchRoute()
  const active = TABS.find((tab) =>
    matchRoute({ to: tab.to, fuzzy: tab.value !== "executions" })
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
