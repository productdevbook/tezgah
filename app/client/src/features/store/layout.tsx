import { StoreTabs } from "@/components/store-tabs"
import { PageHeading } from "@/components/page-heading"
import { useT } from "@/panel/i18n"

/**
 * Chrome shared by every `/store/*` tab. `routes/store.tsx` renders this
 * around an `<Outlet />` the same way `routes/__root.tsx` renders
 * `AppShell` — the layout owns navigation between tabs, and each tab's own
 * screen (`currencies.tsx`, `regions.tsx`, ...) only owns what is in it.
 */
export function StoreLayout({ children }: { children: React.ReactNode }) {
  const t = useT()
  return (
    <div className="space-y-4">
      <PageHeading title={t("nav.store")} subtitle={t("layout.store.why")} />
      <StoreTabs />
      <div className="pt-3">{children}</div>
    </div>
  )
}
