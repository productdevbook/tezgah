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

  "field.name": "Name",
  "field.rate": "Rate",
  "field.starts": "Starts",
  "field.ends": "Ends",
  "field.rules": "Rules",
  "field.priced": "Priced",
  "field.offeredToShoppers": "Offered to shoppers",
  "field.forReturns": "For returns",
  "field.serviceZone": "Service zone",
  "field.shippingProfile": "Shipping profile",
  "field.optionType": "Option type",
  "field.tax": "Tax",
  "field.prices": "Prices",
  "field.workedOut": "Worked out",
  "field.allowed": "Allowed",
  "field.defaultForRegion": "Default for its region",
  "field.combinable": "Combinable",
  "field.taxRegion": "Tax region",
  "field.country": "Country",
  "field.province": "Province",
  "field.parentRegion": "Parent region",
  "field.provider": "Provider",
  "field.amount": "Amount",
  "field.captured": "Captured",
  "field.session": "Session",
  "field.now": "Now",
  "field.issuedWith": "Issued with",
  "field.expires": "Expires",
  "field.disabled": "Disabled",
  "field.issuedOnOrder": "Issued on order",

  "detail.priceList.title": "The list",
  "detail.priceList.why":
    "A sale list marks the price down and says so; an override replaces it silently.",
  "detail.priceList.when": "When it applies",
  "detail.shippingOption.title": "The option",
  "detail.shippingOption.where": "Where it applies",
  "detail.shippingOption.whereWhy":
    "A service zone is the set of addresses this option is offered at; the profile decides which goods it can carry.",
  "detail.region.title": "The region",
  "detail.region.taxWhy":
    "Whether a shown price already contains tax, and who works it out.",
  "detail.region.providers": "Payment providers",
  "detail.taxRate.title": "The rate",
  "detail.taxRate.why":
    "One region has exactly one default rate; a combinable rate stacks on top of whichever applies.",
  "detail.taxRegion.where": "Where it applies",
  "detail.taxRegion.whereWhy":
    "Tax regions nest: a province's rates sit under its country's.",
  "detail.taxRegion.who": "Who works the tax out",
  "detail.payment.what": "What happened to the money",
  "detail.payment.whatWhy":
    "Authorising and capturing are separate acts here, so a payment that exists is not yet a payment that was taken.",
  "detail.payment.where": "Where it sits",
  "detail.giftCard.balance": "The balance",
  "detail.giftCard.origin": "Where it came from",

  "field.symbol": "Symbol",
  "field.nativeSymbol": "Native symbol",
  "field.exponent": "Exponent",
  "field.numericCode": "Numeric code",
  "field.role": "Role",
  "field.password": "Password",
  "field.balance": "Balance",
  "field.usable": "Usable",

  "form.currency.title": "New currency",
  "form.currency.why":
    "The exponent is how many decimal places this currency is written with — a formatting fact, not a multiplier: nothing here is stored in minor units.",
  "form.currency.nativeWhy":
    "What somebody writing in this currency's own language types.",
  "form.currency.exponentWhy":
    "0 to 4. Two for most; zero for a currency with no subunit.",
  "form.currency.numericWhy": "ISO 4217's number for it. Optional.",

  "form.attributes.title": "Shipping and attributes",
  "form.attributes.why":
    "What a carrier needs to quote for this, and what customs needs to let it through.",
  "form.attributes.hsWhy": "What customs calls this kind of thing.",
  "form.attributes.originWhy":
    "Two letters. Where it was made, not where it ships from.",

  "form.promotion.title": "Edit promotion",
  "form.promotion.automatic": "Applied automatically",
  "form.promotion.automaticWhy":
    "An automatic promotion needs no code typed at the till.",
  "form.promotion.usesTotal": "Uses in total",
  "form.promotion.usesTotalWhy":
    "Left empty, there is no limit. Claimed at checkout, not counted at payment.",
  "form.promotion.usesPerCustomer": "Uses per customer",
  "form.promotion.noLimit": "Left empty, there is no limit.",

  "form.organisation.why":
    "What this product belongs to, and what it is called in whatever system it came from.",
  "form.organisation.anId": "An id. Empty clears it.",
  "form.organisation.externalWhy":
    "What this product is called wherever it came from.",

  "form.operator.title": "New operator",
  "form.operator.why":
    "The password is set here and shown to nobody afterwards. To let somebody choose their own, invite them instead — this server sends one when it has a mailer.",
  "form.operator.roleWhy":
    "The first account made is the owner whatever this says — a shop whose only account cannot make a second has locked itself out.",
  "form.operator.passwordWhy":
    "Twelve characters at least. Tell them out of band — nothing here sends a password.",

  "attached.storeCredit": "Store credit",
  "attached.storeCreditWhy":
    "A balance the shop owes them, spent at checkout before any card is.",
  "attached.taxNumbers": "Tax numbers",
  "attached.taxNumbersWhy":
    "Checked is what decides anything — an unchecked number is a string somebody typed.",
  "attached.exemptions": "Exemptions",
  "attached.exemptionsWhy":
    "A certificate that stops tax being charged, in one place and between two dates.",

  "field.state": "State",
  "field.failure": "Failure",
  "field.currencyCode": "Currency code",
  "field.pricesIncludeTax": "Prices include tax",
  "field.autoTax": "Work tax out automatically",

  "detail.workflow.steps": "Steps",
  "detail.workflow.stepsWhy":
    "Each declares how to undo itself, so a failure late in the run walks back through everything before it.",
  "detail.workflow.run": "The run",
  "detail.channel.title": "The channel",
  "detail.shippingProfile.title": "The profile",
  "detail.shippingProfile.why":
    "What a shipping option is allowed to carry — goods that travel together, and goods that cannot.",

  "form.region.new": "New region",
  "form.region.edit": "Edit region",
  "form.region.why":
    "A region is a set of countries sold to in one currency, with one answer about tax.",
  "form.region.currencyWhy":
    "One of the store's currencies. A shop selling in two currencies prices in both rather than converting.",
  "form.region.taxWhy":
    "What a shopper here is shown: a price with tax already in it, or one that gains tax at the till.",
  "form.product.new": "New product",
  "form.product.newWhy":
    "Starts as a draft. Variants, prices and stock go in separately.",

  "field.locale": "Locale",
  "field.disabled2": "Disabled",

  "form.translations.title": "Translations",
  "form.translations.why":
    "What this product is called in another language. The storefront asks for one and falls back to the shop's own.",
  "form.customer.edit": "Edit customer",
  "form.product.edit": "Edit product",
  "form.channel.new": "New sales channel",
  "form.channel.edit": "Edit sales channel",
  "form.channel.why":
    "Where a product is sold: a web shop, an app, a market stall. A product belongs to some of them and not others.",
  "form.channel.disabledWhy":
    "A disabled channel keeps its products and stops selling them.",
  "form.key.title": "Mint a publishable key",
  "form.key.why":
    "A key pins a storefront to the sales channels it may read. The token is shown once, right after this.",
  "form.key.copyNow": "Copy this key now",
  "form.key.copyNowWhy":
    "This will not be shown again. Losing it means minting another.",

  "batch.title": "Import and export",
  "batch.why":
    "A page of variants out as CSV, edited, and back in. Same columns both ways.",
  "batch.export": "Export",
  "batch.exportWhy":
    "One page of variants, flat. A price needs a currency to be a price, so an export naming none leaves those two columns empty.",
  "batch.exported": "The exported rows, as CSV",
  "batch.import": "Import",
  "batch.importWhy":
    "Every row is a variant. A handle that exists is updated; one that does not is created. Nothing is deleted here — that takes an id, and a spreadsheet is the wrong place to name one.",

  "overview.title": "Overview",
  "overview.why":
    "tezgah's admin surface, and how much of it this panel covers.",
  "overview.host": "The host",
  "overview.hostWhy":
    "tezgah is a library. Something else has to mount api::routes().",
  "overview.coverage": "Coverage",
} as const

export type TranslationKey = keyof typeof en
