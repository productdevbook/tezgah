import { Link, type LinkComponentProps } from "@tanstack/react-router"

/**
 * A link to a section, from its slug.
 *
 * The router's route union is closed, so `to` cannot be built from a runtime
 * string — hence the switch. It lives here once because three places want it:
 * the sidebar, the command palette, and whatever comes next. A slug with no
 * screen falls through to `/$section`, which draws what is not built yet.
 */
export function SectionLink({
  slug,
  ...props
}: { slug: string } & Omit<LinkComponentProps<"a">, "to" | "params">) {
  switch (slug) {
    case "products":
      return <Link to="/products" {...props} />
    case "orders":
      return <Link to="/orders" {...props} />
    case "inventory":
      return <Link to="/inventory" {...props} />
    case "customers":
      return <Link to="/customers" {...props} />
    case "promotions":
      return <Link to="/promotions" {...props} />
    case "subscriptions":
      return <Link to="/subscriptions" {...props} />
    case "store":
      return <Link to="/store" {...props} />
    case "payouts":
      return <Link to="/payouts" {...props} />
    case "workflows":
      return <Link to="/workflows" {...props} />
    case "baskets":
      return <Link to="/baskets" {...props} />
    case "fulfilment":
      return <Link to="/fulfilment" {...props} />
    case "tax":
      return <Link to="/tax" {...props} />
    case "pricing":
      return <Link to="/pricing" {...props} />
    case "payments":
      return <Link to="/payments" {...props} />
    case "credit":
      return <Link to="/credit" {...props} />
    case "carts":
      return <Link to="/carts" {...props} />
    case "operators":
      return <Link to="/operators" {...props} />
    case "batch":
      return <Link to="/batch" {...props} />
    case "records":
      return <Link to="/records" {...props} />
    default:
      return <Link to="/$section" params={{ section: slug }} {...props} />
  }
}
