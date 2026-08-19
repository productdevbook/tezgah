import { Navigate } from "@tanstack/react-router"

import { sectionBySlug } from "@/lib/nav"
import { Inventory } from "@/screens/inventory"
import { Customers } from "@/screens/customers"
import { NotBuilt } from "@/screens/not-built"
import { Orders } from "@/screens/orders"
import { Products } from "@/screens/products"
import { Promotions } from "@/screens/promotions"
import { Store } from "@/screens/store"
import { Subscriptions } from "@/screens/subscriptions"

const BUILT: Record<string, () => React.ReactElement> = {
  products: Products,
  orders: Orders,
  inventory: Inventory,
  customers: Customers,
  promotions: Promotions,
  subscriptions: Subscriptions,
  store: Store,
}

/**
 * One component behind every section, rather than one route each.
 *
 * The sections come from the same table the sidebar reads, so a menu entry
 * that leads to a blank page cannot happen: a slug with no screen renders what
 * is missing, and a slug that is not a section at all goes home.
 */
export function SectionScreen({ slug }: { slug: string }) {
  const section = sectionBySlug(slug)
  if (!section) return <Navigate to="/" replace />

  const Screen = BUILT[section.slug]
  return Screen ? <Screen /> : <NotBuilt section={section} />
}
