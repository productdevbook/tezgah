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

  "screen.products.title": "Products",
  "screen.products.subtitle":
    "This surface sees every status. The storefront sees published only.",
  "screen.products.empty": "No products",
  "screen.products.emptyAny": "Nothing in the catalogue yet.",
  "screen.products.emptyStatus": "Nothing with status {status}.",

  "screen.orders.title": "Orders",
  "screen.orders.subtitle": "Drafts are listed too, and say so.",
  "screen.orders.empty": "No orders",
  "screen.orders.emptyAny": "Nothing has been placed yet.",

  "screen.customers.title": "Customers",
  "screen.customers.subtitle":
    "Guests are customers too — a cart makes one before an account does.",
  "screen.customers.empty": "No customers",
  "screen.customers.emptyAny": "Nobody has shopped yet.",

  "screen.inventory.title": "Inventory",
  "screen.inventory.subtitle":
    "An item is the thing counted. What is on hand is counted per location, one level down.",
  "screen.inventory.empty": "Nothing stocked",
  "screen.inventory.emptyAny": "No inventory item exists yet.",

  "screen.carts.title": "Carts",
  "screen.carts.subtitle":
    "Every cart the store holds, abandoned ones included.",
  "screen.carts.empty": "No carts",
  "screen.carts.emptyAny": "Nobody has started one yet.",

  "screen.promotions.title": "Promotions",
  "screen.promotions.subtitle":
    "A use is claimed when a cart is checked out, not when it is paid for.",
  "screen.promotions.empty": "No promotions",
  "screen.promotions.emptyAny": "Nothing is on offer.",

  "screen.subscriptions.title": "Subscriptions",
  "screen.subscriptions.empty": "No subscriptions",
  "screen.subscriptions.emptyAny": "Nothing recurring is sold.",

  "screen.credit.title": "Credit",
  "screen.credit.subtitle":
    "Gift cards. What a customer keeps on account is read from their own record.",
  "screen.credit.empty": "No gift cards",
  "screen.credit.emptyAny": "None has been issued yet.",

  "search.nothingMatches": "Nothing matches {q}.",

  "field.id": "ID",
  "field.title": "Title",
  "field.created": "Created",
  "field.updated": "Updated",
  "field.email": "Email",
  "field.firstName": "First name",
  "field.lastName": "Last name",
  "field.phone": "Phone",
  "field.company": "Company",
  "field.sku": "SKU",
  "field.account": "Account",
  "field.erased": "Erased",
  "field.ships": "Ships",

  "value.yes": "Yes",
  "value.no": "No",
  "value.registered": "Registered",
  "value.guest": "Guest",
  "value.shipped": "Shipped",
  "value.digital": "Digital, no shipping",

  "detail.customer.title": "Who they are",
  "detail.customer.empty": "No customer",
  "detail.customer.account": "Account",
  "detail.inventory.title": "The item",
  "detail.inventory.empty": "No inventory item",
  "detail.nothingToShow": "Nothing to show.",

  "field.handle": "Handle",
  "field.subtitle": "Subtitle",
  "field.description": "Description",
  "field.discountable": "Discountable",
  "field.rejectedReason": "Rejected reason",
  "field.productType": "Product type",
  "field.collection": "Collection",
  "field.externalId": "External ID",
  "field.thumbnail": "Thumbnail",
  "field.weight": "Weight",
  "field.length": "Length",
  "field.height": "Height",
  "field.width": "Width",
  "field.material": "Material",
  "field.hsCode": "HS code",
  "field.originCountry": "Origin country",
  "field.variantId": "Variant id",

  "detail.product.general": "General",
  "detail.product.organisation": "Organisation",
  "detail.product.media": "Media",
  "detail.product.shipping": "Shipping",
  "detail.product.shippingWhy":
    "What a carrier needs to quote, and what customs needs to let it through.",
  "detail.product.digital": "Digital content",
  "detail.product.digitalWhy":
    "A file belongs to one variant — take an id from the variants above to see or add what it carries.",

  "field.number": "Number",
  "field.currency": "Currency",
  "field.version": "Version",
  "field.order": "Order",
  "field.payment": "Payment",
  "field.fulfilment": "Fulfilment",
  "field.draft": "Draft",
  "field.canceled": "Canceled",
  "field.completed": "Completed",
  "field.basket": "Basket",
  "field.paymentCollection": "Payment collection",

  "detail.order.whereItStands": "Where it stands",
  "detail.order.whereItStandsWhy":
    "Three statuses that move independently — the order, its money and its parcels — so none of them is folded into the others.",
  "detail.order.whoFor": "Who it is for",
  "detail.order.attachedTo": "What it is attached to",
  "detail.order.basketWhy":
    "A basket is a marketplace checkout: one payment across several sellers, one order each.",
  "detail.order.versionWhy":
    "The version rises with every edit; earlier versions keep what the order looked like then.",
  "detail.order.entitlements": "Entitlements",
  "detail.order.entitlementsWhy":
    "What this order granted a right to, and whether that right still stands.",

  "field.status": "Status",
  "field.customer": "Customer",
  "field.cycle": "Cycle",
  "field.nextBilling": "Next billing",
  "field.currentPeriod": "Current period",
  "field.endsThisPeriod": "Ends this period",
  "field.ended": "Ended",
  "field.dunningAttempts": "Dunning attempts",
  "field.sellingPlan": "Selling plan",
  "field.code": "Code",
  "field.kind": "Kind",
  "field.applied": "Applied",
  "field.used": "Used",
  "field.perCustomer": "Per customer",
  "field.campaign": "Campaign",

  "detail.subscription.billed": "What is being billed",
  "detail.subscription.cycle": "The cycle",
  "detail.subscription.dunningWhy":
    "Above zero means a charge failed and is being retried — a different thing from a cancelled contract, which the status says instead.",
  "detail.subscription.who": "Who and what",

  "detail.basket.orders": "Orders",
  "detail.basket.ordersWhy":
    "One basket becomes one order per seller — the payment is single, the fulfilment is not.",
  "detail.basket.carts": "Carts",
  "detail.basket.cartsWhy":
    "A seller's own leg of the checkout, before it became an order.",
  "detail.basket.payment": "The payment",

  "detail.promotion.title": "The promotion",
  "detail.promotion.left": "How much is left",
  "detail.promotion.leftWhy":
    "Claimed at checkout rather than counted at payment, so this is what is spoken for.",
} as const

export type TranslationKey = keyof typeof en
