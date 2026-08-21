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

  "general.nothingToShow": "Nothing to show.",
  "state.noHost": "No host is answering",
  "state.noHostWhy":
    "tezgah is a library and serves nothing itself. Point VITE_TEZGAH_API at whatever mounts api::routes(), or run the example shop.",
  "state.noToken": "This panel has no token",
  "state.noTokenWhy":
    "The admin surface wants one and nothing was sent. Connect the panel with the server's ADMIN_TOKEN.",
  "state.refused": "Refused",
  "state.refusedWhy":
    "The token was sent and the server said no — wrong token, or one the server no longer holds. It never says which rows exist, so there is nothing more to read into that.",
  "state.notHere": "Not here",
  "state.notHereWhy": "The host answered, and has no such row.",
  "state.refusedRequest": "The request did not go through",
  "state.drifted": "The panel and the crate disagree",
  "state.driftedWhy":
    "The host answered, and the answer is not the shape this panel expects. Its types are transcribed from the Rust by hand, so the crate has moved and this has not.",
  "empty.basket": "No basket",
  "empty.orders": "No orders",
  "empty.ordersWhy": "This basket has not split into an order yet.",
  "empty.carts": "No carts",
  "empty.cartsWhy":
    "No seller-scope has an open leg of this checkout right now.",
  "frame.rejected": "Rejected",
  "frame.rejectedWhy":
    "By row number, counting from the first row under the header.",
  "empty.giftCard": "No gift card",
  "empty.customer": "No customer",
  "empty.carriers": "No carriers",
  "empty.carriersWhy": "Nothing ships until a shop turns a provider on.",
  "frame.carriers": "Carriers",
  "frame.fulfilmentSets": "Fulfilment sets",
  "frame.fulfilmentSetsWhy":
    "A set groups the service zones one carrier serves.",
  "empty.fulfilmentSets": "No fulfilment sets",
  "empty.fulfilmentSetsWhy":
    "A set groups the service zones a location or a store ships through.",
  "empty.shippingOption": "No shipping option",
  "frame.optionTypes": "Option types",
  "frame.optionTypesWhy":
    "The labels a shopper picks between — standard, express — shared across options.",
  "empty.shippingOptionTypes": "No shipping option types",
  "frame.shippingOptions": "Shipping options",
  "frame.shippingOptionsWhy":
    "What a shopper can choose at the till, and what each costs.",
  "empty.shippingOptions": "No shipping options",
  "empty.shippingOptionsWhy":
    "A service zone offers nothing to ship with until one is added.",
  "empty.shippingProfile": "No shipping profile",
  "frame.shippingProfiles": "Shipping profiles",
  "frame.shippingProfilesWhy":
    "What an option is allowed to carry: goods that travel together, and goods that cannot.",
  "empty.shippingProfiles": "No shipping profiles",
  "empty.shippingProfilesWhy":
    "A product ships under a profile, which decides which options fit it.",
  "frame.invited": "Invited",
  "frame.invitedWhy":
    "Sent and not yet accepted. Inviting the same address again replaces the link rather than adding a second.",
  "empty.accounts": "No accounts",
  "empty.accountsWhy":
    "Only the admin token can reach this back office. Make an account.",
  "frame.accounts": "Accounts",
  "frame.accountsWhy":
    "Disabling one ends every session it holds, in the same transaction.",
  "empty.order": "No order",
  "empty.entitlements": "No entitlements",
  "empty.entitlementsWhy": "This order carries no digital rights.",
  "empty.payment": "No payment",
  "frame.payments": "Payments",
  "frame.paymentsWhy":
    "Authorising and capturing are separate acts, so a payment that exists is not yet money taken.",
  "empty.payments": "No payments",
  "empty.paymentsWhy": "Nothing has been taken yet.",
  "frame.refundReasons": "Refund reasons",
  "frame.refundReasonsWhy":
    "The reasons a refund can be given against, kept so a report can count them.",
  "empty.refundReasons": "No refund reasons",
  "empty.refundReasonsWhy":
    "A refund can be given without one, but nothing here explains why yet.",
  "frame.commissionRules": "Commission rules",
  "frame.commissionRulesWhy":
    "What the marketplace keeps from a seller's line, and on what.",
  "empty.commissionRules": "No commission rules",
  "empty.commissionRulesWhy":
    "A category with no rule and no default earns no commission — nothing is taken until one is set.",
  "empty.payouts": "No payouts",
  "empty.payoutsWhy": "Nothing has been recorded as paid out yet.",
  "empty.balance": "No balance",
  "empty.balanceWhy": "Nothing in this currency.",
  "empty.payoutLines": "No payout lines",
  "empty.payoutLinesWhy": "Nothing earned on this order yet.",
  "empty.priceList": "No price list",
  "frame.priceLists": "Price lists",
  "frame.priceListsWhy":
    "Dated or conditional prices — a sale that says so, or an override that does not.",
  "empty.priceLists": "No price lists",
  "empty.priceListsWhy":
    "A price list overrides a price set's own prices for a rule it matches.",
  "empty.pricePreference": "No preference set",
  "empty.pricePreferenceWhy":
    "Nothing decides this attribute's tax display yet.",
  "empty.priceSet": "No price set",
  "empty.prices": "No prices",
  "empty.pricesWhy": "This price set has no prices yet.",
  "frame.prices": "Prices",
  "frame.pricesWhy":
    "Type an amount and save them together. Only the amount is editable — a currency or a quantity band is what makes a price that price.",
  "empty.product": "No product",
  "empty.digitalContent": "No digital content",
  "empty.digitalContentWhy": "This variant carries no files yet.",
  "empty.promotion": "No promotion",
  "empty.audit": "Nothing written down yet",
  "empty.auditWhy": "An audit row is written when something changes.",
  "frame.audit": "Audit",
  "frame.auditWhy":
    "Who did what to which row. An ADMIN_TOKEN request names nobody, because a shared secret is not a person.",
  "empty.events": "Nothing to say yet",
  "empty.eventsWhy":
    "An event is written when something worth telling happens.",
  "frame.outbox": "Outbox",
  "empty.currencies": "No currencies",
  "empty.currenciesWhy":
    "Nothing prices or opens a cart until a shop enables one.",
  "frame.currencies": "Currencies",
  "frame.currenciesWhy":
    "The exponent is how a currency is written, not a multiplier — nothing here is stored in minor units.",
  "frame.keys": "Publishable keys",
  "frame.keysWhy":
    "A key pins a storefront to the channels it may read. The token is shown once, when it is minted.",
  "empty.keys": "No publishable keys",
  "empty.keysWhy":
    "What a storefront sends as x-publishable-key. Shown once when minted.",
  "empty.region": "No region",
  "frame.regions": "Regions",
  "frame.regionsWhy":
    "A region is a set of countries sold to in one currency, with one answer about tax.",
  "empty.regions": "No regions",
  "empty.regionsWhy": "A region decides currency and how tax is shown.",
  "empty.salesChannel": "No sales channel",
  "frame.salesChannels": "Sales channels",
  "frame.salesChannelsWhy":
    "Where a product is sold. A product belongs to some channels and not others.",
  "empty.salesChannels": "No sales channels",
  "empty.salesChannelsWhy":
    "A channel decides which products a storefront can see.",
  "empty.subscription": "No subscription",
  "empty.taxRate": "No tax rate",
  "frame.taxRates": "Tax rates",
  "frame.taxRatesWhy":
    "One default per region, and combinable rates that stack on top.",
  "empty.taxRates": "No tax rates",
  "empty.taxRatesWhy": "A region charges nothing until a rate is set.",
  "empty.taxRegion": "No tax region",
  "frame.taxRegions": "Tax regions",
  "frame.taxRegionsWhy": "Nested: a province's rates sit under its country's.",
  "empty.taxRegions": "No tax regions",
  "empty.taxRegionsWhy":
    "A country or province with no region here charges no tax.",
  "empty.registrations": "No registrations",
  "empty.registrationsWhy":
    "The shop has recorded nowhere it is registered to file tax.",
  "frame.registrations": "Registrations",
  "frame.registrationsWhy":
    "Where the shop is registered to collect, and under which number.",
  "frame.deadLetters": "Dead letters",
  "frame.deadLettersWhy":
    "Runs that ran out of retries. Nothing retries these again on its own.",
  "empty.deadLetters": "No dead letters",
  "empty.deadLettersWhy":
    "Nothing has run out of retries and been given up on.",
  "empty.run": "No run",
  "empty.steps": "No steps",
  "empty.stepsWhy": "This workflow declared none.",
  "empty.runs": "No runs",
  "frame.outboxWhy":
    "What the shop has to say. A destination turns these into signed posts; with none configured they sit here to be read.",

  "field.no": "No",
  "field.default": "Default",
  "field.dunning": "Dunning",
  "field.inStore": "In store",
  "field.initialBalance": "Initial balance",
  "field.label": "Label",
  "field.lastUsed": "Last used",
  "field.nextCharge": "Next charge",
  "field.payout": "Payout",
  "field.placed": "Placed",
  "field.priceType": "Price type",
  "field.providers": "Providers",
  "field.quantity": "Quantity",
  "field.reference": "Reference",
  "field.referenceId": "Reference id",
  "field.region": "Region",
  "field.return": "Return",
  "field.run": "Run",
  "field.scope": "Scope",
  "field.since": "Since",
  "field.started": "Started",
  "field.step": "Step",
  "field.transactionKey": "Transaction key",
  "field.value": "Value",
  "field.when": "When",
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

  "layout.workflows.why":
    "Every run the runner has driven, and every step it could not finish.",
  "layout.tax.why":
    "What is charged where, and what the shop itself is registered under.",
  "layout.store.why": "Where the shop sells, and through what.",
  "layout.pricing.why":
    "Lists, one preference, and the sets and rows behind a price.",
  "layout.payouts.why":
    "What a seller-scope is owed, and the commission that decides it.",
  "layout.payments.why":
    "What was taken against an order, and why a refund might be given.",
  "layout.fulfilment.why":
    "Who carries it, what it ships in, and what a shop charges to send it.",

  "table.chooseEvery": "Choose every row on this page",
  "table.chooseThis": "Choose this row",
  "actions.menu": "Actions",

  "field.priceSetId": "Price set id",
  "field.orderId": "Order id",

  "section.media": "Media",
  "section.mediaWhy":
    "An address — uploaded here if this shop stores files, or wherever it is already served from.",
  "section.executions": "Executions",
  "section.executionsWhy": "Every workflow run the runner has driven.",
  "section.taxRules": "What it applies to",
  "section.taxRulesWhy":
    "A rule narrows the rate to one kind of thing. With none, it applies to everything in its region.",
  "section.variants": "Variants",
  "section.variantsWhy":
    "The thing with a price and a count. A product with none cannot be bought.",
  "section.levels": "What is where",
  "section.levelsWhy":
    "Counted per location. Type a count and save them together — one call, so a shelf is counted at once or not at all.",
  "section.movements": "What moved",
  "section.movementsWhy":
    "Every add and every spend. A balance is the sum of these, not a number somebody set.",

  "screen.subscriptions.subtitle":
    "A contract, not an order. The orders it produces are listed under Orders.",
  "screen.records.subtitle": "The audit trail and the outbox, newest first.",
  "screen.operators.subtitle":
    "An account belongs to a person and can be revoked. The admin token belongs to nobody and cannot.",
  "screen.baskets.subtitle":
    "Baskets are reached by id; the crate has no list.",
} as const

export type TranslationKey = keyof typeof en
