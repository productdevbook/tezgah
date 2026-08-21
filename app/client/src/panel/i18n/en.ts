/**
 * Every string the panel draws, flat and dotted.
 *
 * Flat rather than nested so a key is `keyof typeof en` — one type, no
 * recursion — and every other locale is `Record<keyof typeof en, string>`,
 * which makes a missing translation a compile error rather than an English
 * word on a Turkish screen.
 */
export const en = {
  "actions.cancel": "Cancel",
  "actions.save": "Save",
  "actions.create": "Create",
  "actions.edit": "Edit",
  "actions.delete": "Delete",
  "actions.close": "Close",
  "actions.back": "Back",
  "actions.continue": "Continue",
  "actions.retry": "Try again",
  "actions.saving": "Saving…",

  "general.areYouSure": "Are you sure?",
  "general.unsavedTitle": "You have unsaved changes",
  "general.unsavedDescription":
    "Leaving now discards what you have typed. This cannot be undone.",
  "general.noValue": "—",
  "general.metadata": "Metadata",
  "general.json": "JSON",
  "general.details": "Details",
  "general.loading": "Loading…",
  "general.empty": "Nothing here yet",
  "general.of": "of",

  "error.unreachable": "No host answered.",
  "error.unauthenticated": "This panel is not connected.",
  "error.denied": "The host refused this.",
  "error.notFound": "Not found.",
  "error.refused": "The host refused this request.",
  "error.drifted": "The host answered with something this panel cannot read.",

  "nav.group.selling": "Selling",
  "nav.group.orders": "Orders",
  "nav.group.gettingItThere": "Getting it there",
  "nav.group.money": "Money",
  "nav.group.theShop": "The shop",
  "nav.group.thisServer": "This server",

  "nav.products": "Products",
  "nav.pricing": "Pricing",
  "nav.promotions": "Promotions",
  "nav.orders": "Orders",
  "nav.baskets": "Baskets",
  "nav.carts": "Carts",
  "nav.subscriptions": "Subscriptions",
  "nav.inventory": "Inventory",
  "nav.fulfilment": "Fulfilment",
  "nav.payments": "Payments",
  "nav.credit": "Credit",
  "nav.payouts": "Payouts",
  "nav.tax": "Tax",
  "nav.customers": "Customers",
  "nav.store": "Store",
  "nav.digital": "Digital",
  "nav.workflows": "Workflows",
  "nav.operators": "Operators",
  "nav.batch": "Import and export",
  "nav.records": "What happened",

  "nav.soon": "soon",
  "nav.overview": "Overview",
  "nav.goTo": "Go to…",
  "nav.disconnect": "Disconnect",
  "nav.adminToken": "Admin token — not a person",
  "nav.coverage": "{covered} of {operations} admin operations have a screen",

  "table.back": "Back",
  "table.next": "Next",
  "table.chosen": "{count} chosen",
  "table.showing": "{shown} of {total}",
} as const

export type TranslationKey = keyof typeof en
