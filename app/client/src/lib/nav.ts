/**
 * The panel's navigation, and what stands behind each entry.
 *
 * `operations` is what `tests/snapshots/openapi.json` declares for that tag on
 * the admin surface — the number is here so a screen that covers three of
 * thirty-seven cannot quietly read as finished.
 */
export type Section = {
  slug: string
  title: string
  /** The OpenAPI tag whose operations this section is built over. */
  tag: string
  operations: number
  /** False until a screen exists; the route renders what is missing instead. */
  built: boolean
  /**
   * Where this domain's operations are drawn, when it is not a place of its
   * own. `digital` is the case that made this field exist: its eight
   * operations all hang off another record — `GET /admin/orders/{id}/
   * entitlements`, `GET /admin/variants/{id}/digital-content` — so a
   * standalone Digital screen would be a claim about the domain's shape that
   * the route table does not make. It stays counted, because the operations
   * are real and somebody has to draw them; it leaves the sidebar, because
   * there is nowhere for that entry to go.
   */
  folded?: string
}

export type Group = {
  title: string
  sections: Section[]
}

export const GROUPS: Group[] = [
  {
    title: "Selling",
    sections: [
      {
        slug: "products",
        title: "Products",
        tag: "catalogue",
        operations: 79,
        built: true,
      },
      {
        slug: "pricing",
        title: "Pricing",
        tag: "pricing",
        operations: 23,
        built: true,
      },
      {
        slug: "promotions",
        title: "Promotions",
        tag: "promotion",
        operations: 19,
        built: true,
      },
    ],
  },
  {
    title: "Orders",
    sections: [
      {
        slug: "orders",
        title: "Orders",
        tag: "order",
        operations: 112,
        built: true,
      },
      {
        slug: "baskets",
        title: "Baskets",
        tag: "order_basket",
        operations: 5,
        built: true,
      },
      {
        slug: "carts",
        title: "Carts",
        tag: "cart",
        operations: 11,
        built: true,
      },
      {
        slug: "subscriptions",
        title: "Subscriptions",
        tag: "subscription",
        operations: 26,
        built: true,
      },
    ],
  },
  {
    title: "Getting it there",
    sections: [
      {
        slug: "inventory",
        title: "Inventory",
        tag: "inventory",
        operations: 37,
        built: true,
      },
      {
        slug: "fulfilment",
        title: "Fulfilment",
        tag: "fulfilment",
        operations: 35,
        built: true,
      },
    ],
  },
  {
    title: "Money",
    sections: [
      {
        slug: "payments",
        title: "Payments",
        tag: "payment",
        operations: 19,
        built: true,
      },
      {
        slug: "credit",
        title: "Credit",
        tag: "credit",
        operations: 17,
        built: true,
      },
      {
        slug: "payouts",
        title: "Payouts",
        tag: "payout",
        operations: 7,
        built: true,
      },
      { slug: "tax", title: "Tax", tag: "tax", operations: 24, built: true },
    ],
  },
  {
    title: "The shop",
    sections: [
      {
        slug: "customers",
        title: "Customers",
        tag: "customer",
        operations: 25,
        built: true,
      },
      {
        slug: "store",
        title: "Store",
        tag: "store",
        operations: 32,
        built: true,
      },
      {
        slug: "digital",
        title: "Digital",
        tag: "digital",
        operations: 8,
        built: true,
        folded: "the order's entitlements and the product's digital content",
      },
      {
        slug: "workflows",
        title: "Workflows",
        tag: "workflow",
        operations: 4,
        built: true,
      },
    ],
  },
]

export const SECTIONS: Section[] = GROUPS.flatMap((g) => g.sections)

/**
 * The ones with a place of their own. A folded domain is counted and drawn,
 * but not from the sidebar — there is no address for it to lead to.
 */
export const NAVIGABLE: Section[] = SECTIONS.filter((s) => !s.folded)

export function sectionBySlug(slug: string): Section | undefined {
  return SECTIONS.find((s) => s.slug === slug)
}

/** What the admin surface declares in total, against what has a screen. */
export const COVERAGE = {
  operations: SECTIONS.reduce((n, s) => n + s.operations, 0),
  covered: SECTIONS.filter((s) => s.built).reduce(
    (n, s) => n + s.operations,
    0
  ),
}

/**
 * The host's own screens, kept apart from `GROUPS` on purpose.
 *
 * tezgah authenticates nobody and declares no route for an account, so an
 * operator is not one of the operations `COVERAGE` counts. Folding it in
 * would make the tally read as progress against a surface it is not part of.
 *
 * Import and export is here for a different reason: its four routes *are*
 * tezgah's, and they are counted under `catalogue` where they belong. What
 * is not a domain is the screen — a shop does not think of "the batch
 * endpoints", it thinks of getting its prices out and back in.
 */
export const SERVER_SECTIONS: { slug: string; title: string }[] = [
  { slug: "operators", title: "Operators" },
  { slug: "batch", title: "Import and export" },
]
