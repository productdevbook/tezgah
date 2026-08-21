import type { TranslationKey } from "@/panel/i18n"

/**
 * The panel's navigation, and what stands behind each entry.
 *
 * `operations` is what `tests/snapshots/openapi.json` declares for that tag on
 * the admin surface — the number is here so a screen that covers three of
 * thirty-seven cannot quietly read as finished.
 */
export type Section = {
  slug: string
  /**
   * A key, not a word. The sidebar is the one place every screen shows, so an
   * English title here would be an English word on a Turkish panel however
   * well the rest was translated — and the dictionary's `Record<TranslationKey,
   * string>` makes a missing Turkish entry a compile error rather than that.
   */
  title: TranslationKey
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
  title: TranslationKey
  sections: Section[]
}

export const GROUPS: Group[] = [
  {
    title: "nav.group.selling",
    sections: [
      {
        slug: "products",
        title: "nav.products",
        tag: "catalogue",
        operations: 79,
        built: true,
      },
      {
        slug: "pricing",
        title: "nav.pricing",
        tag: "pricing",
        operations: 23,
        built: true,
      },
      {
        slug: "promotions",
        title: "nav.promotions",
        tag: "promotion",
        operations: 19,
        built: true,
      },
    ],
  },
  {
    title: "nav.group.orders",
    sections: [
      {
        slug: "orders",
        title: "nav.orders",
        tag: "order",
        operations: 112,
        built: true,
      },
      {
        slug: "baskets",
        title: "nav.baskets",
        tag: "order_basket",
        operations: 5,
        built: true,
      },
      {
        slug: "carts",
        title: "nav.carts",
        tag: "cart",
        operations: 11,
        built: true,
      },
      {
        slug: "subscriptions",
        title: "nav.subscriptions",
        tag: "subscription",
        operations: 26,
        built: true,
      },
    ],
  },
  {
    title: "nav.group.gettingItThere",
    sections: [
      {
        slug: "inventory",
        title: "nav.inventory",
        tag: "inventory",
        operations: 37,
        built: true,
      },
      {
        slug: "fulfilment",
        title: "nav.fulfilment",
        tag: "fulfilment",
        operations: 35,
        built: true,
      },
    ],
  },
  {
    title: "nav.group.money",
    sections: [
      {
        slug: "payments",
        title: "nav.payments",
        tag: "payment",
        operations: 19,
        built: true,
      },
      {
        slug: "credit",
        title: "nav.credit",
        tag: "credit",
        operations: 17,
        built: true,
      },
      {
        slug: "payouts",
        title: "nav.payouts",
        tag: "payout",
        operations: 7,
        built: true,
      },
      {
        slug: "tax",
        title: "nav.tax",
        tag: "tax",
        operations: 24,
        built: true,
      },
    ],
  },
  {
    title: "nav.group.theShop",
    sections: [
      {
        slug: "customers",
        title: "nav.customers",
        tag: "customer",
        operations: 25,
        built: true,
      },
      {
        slug: "store",
        title: "nav.store",
        tag: "store",
        operations: 32,
        built: true,
      },
      {
        slug: "digital",
        title: "nav.digital",
        tag: "digital",
        operations: 8,
        built: true,
        folded: "the order's entitlements and the product's digital content",
      },
      {
        slug: "workflows",
        title: "nav.workflows",
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
export const SERVER_SECTIONS: { slug: string; title: TranslationKey }[] = [
  { slug: "operators", title: "nav.operators" },
  { slug: "batch", title: "nav.batch" },
  { slug: "records", title: "nav.records" },
]
