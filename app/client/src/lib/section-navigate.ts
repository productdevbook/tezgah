import { useNavigate } from "@tanstack/react-router"

/**
 * The same switch, for somewhere a link cannot go — the command palette,
 * where cmdk answers a keyboard Enter with `onSelect` rather than a click.
 */
export function useSectionNavigate() {
  const navigate = useNavigate()

  return (slug: string) => {
    switch (slug) {
      case "products":
        return void navigate({ to: "/products" })
      case "orders":
        return void navigate({ to: "/orders" })
      case "inventory":
        return void navigate({ to: "/inventory" })
      case "customers":
        return void navigate({ to: "/customers" })
      case "promotions":
        return void navigate({ to: "/promotions" })
      case "subscriptions":
        return void navigate({ to: "/subscriptions" })
      case "store":
        return void navigate({ to: "/store" })
      case "payouts":
        return void navigate({ to: "/payouts" })
      case "workflows":
        return void navigate({ to: "/workflows" })
      case "baskets":
        return void navigate({ to: "/baskets" })
      case "fulfilment":
        return void navigate({ to: "/fulfilment" })
      case "tax":
        return void navigate({ to: "/tax" })
      case "pricing":
        return void navigate({ to: "/pricing" })
      case "payments":
        return void navigate({ to: "/payments" })
      case "credit":
        return void navigate({ to: "/credit" })
      case "carts":
        return void navigate({ to: "/carts" })
      default:
        return void navigate({ to: "/$section", params: { section: slug } })
    }
  }
}
