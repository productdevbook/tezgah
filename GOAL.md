# Goal

Everything a commerce engine does, in Rust, as a library.

The measure is concrete: a shop built on tezgah should not have to drop to raw
SQL for anything a shop normally does, and anyone moving from an established
platform should not find a hole where a feature they relied on used to be.

Medusa's commerce surface is the yardstick because it is the most complete open
one — fifteen commerce modules, read at v2.19.0. Every one of them is answered
below. What is *not* answered is listed too, with the reason, so a gap is a
decision somebody can find rather than something forgotten.

## What a host provides instead

tezgah is a library. Some of what a platform ships is not commerce and is
already solved by whatever embeds this, so it is asked for through
[`ports`](src/ports.rs) rather than built:

| Not built | Asked for as | Why |
|---|---|---|
| authentication, accounts, sessions | `Actor` | a host already knows who is signed in |
| roles and permissions | `Authorizer` | a host already has an engine; tezgah asks it |
| event delivery, message bus | `EventSink` | an outbox row is written; delivery is the host's |
| background workers | `Jobs` | tezgah enqueues in your transaction; you run them |
| e-mail, SMS, push | `EventSink` | tezgah says `order.paid`; the host writes the letter |
| file and image storage | a URL on the record | a host already has media |
| caching | — | a library that caches behind your back is a bug |
| analytics dashboards | the API | reporting over Postgres is the host's |
| the admin screens | the API | the crate ships no UI; [`app/client/`](app/client) is a panel over it, built and released apart |

## Where this is

The commerce engine is the part that is far along; the host half around it is
not, and stage 16 below is that list. `docs/architecture.md` is where each of
those gaps is measured and assigned to a layer — the library, the binary in
`app/server`, or the panel in `app/client`.

Tests green. Both API surfaces are served, generated into a snapshotted OpenAPI
document, and every domain in stages 2 to 13 is module, schema and test
complete and reachable from a route.

**Medusa's commerce surface has been swept model by model and field by field**,
against v2.19.0's own source rather than from memory. All 36 of its modules are
answered: 22 by a domain here, 9 by a port a host implements, 3 deliberately
absent (search, the module-isolation link tables, admin-UI preferences), and
translation by `catalogue`'s per-locale content plus the three entities a
shopper reads. The sweep found 17 gaps. They were filed one by one and closed.

The permission matrix that was once a gap now proves the rule for effectively
the whole route table: every route called against a host that denies everything
must come back denied, with a completeness check that fails the build if a route
is neither called nor named. Its `TOLERATED` list is down to two entries, both
needing a live provider fixture to construct an argument. A second matrix does
the same for ownership: a storefront route must refuse another shopper's row and
serve its own.

Two things that were true of every one of those operations until now: nothing
served them, and nothing drew them. [`examples/shop`](examples/shop) is a shop
that runs — axum over the route table, all five ports implemented including a
job worker that actually runs what it enqueues, and a `PaymentProvider` built
over `dyn kasapay_core::Provider`, which is the first thing outside that
module's own tests to lean on the mapping. It binds 6 of the 486 operations,
enough to walk browse → cart → checkout → order, and says so. [`app/client/`](app/client)
is an admin panel over the same surface, with screens for products, orders and
inventory — 228 of the 486 operations behind a screen, and every section that
has none saying how many it is not drawing. Neither is in the crate: depending
on tezgah pulls in no axum and no React.

What is left is written as issues rather than as boxes here. #53 — the payment
providers moving onto kasapay — closed both gaps it was filed waiting on:
kasapay#149 opened `Currency` from nine variants to 119, and kasapay#150 gave
`Provider::charge` a route for the hosted checkout form iyzico's most common
flow uses. `src/providers/stripe.rs` and `iyzico.rs`, tezgah's own hosted-flow
adapters written before kasapay existed, are gone with them; `src/providers/`
holds only the kasapay mapping now, kept ready for whichever host wires it up.

## Stages

Nothing starts before the thing above it works.

### 1. Foundation

- [x] `ports` — authorization, audit, events, jobs, clock
- [x] `Money` — `Decimal` and a currency; `allocate` whose parts add back up
- [x] `Error` — one canonical struct, private cause, stable `code()`
- [x] `id` — typed ids, so a product id cannot be passed where an order id goes
- [x] `scope` — the column, the RLS policies, a schema test no table escapes
- [x] test harness — real Postgres per test, two scopes seeded, parallel-safe

### 2. Workflow runner

Checkout is not one transaction: it reserves stock, asks a provider for money,
writes an order, and the provider is not in your database.

- [x] `Step` — invoke, and how to undo what invoking did
- [x] compensation — a later failure walks back through the earlier steps
- [x] `workflow_execution` — checkpoints, claimed by `FOR UPDATE SKIP LOCKED`
- [x] idempotency — the same transaction id twice resumes rather than repeats
- [x] retry with backoff, a ceiling, a dead letter
- [x] timeouts, and a lease a long step extends while it runs
- [x] advisory locks, so two checkouts on one cart do not interleave
- [x] nested workflows, and steps that run in parallel
- [x] tests: interrupt at every step in turn, assert nothing is left behind

### 3. Store, currency, region, sales channel

- [x] store, its default currency and its supported ones
- [x] currency, with the exponent used for rounding and display
- [x] region: countries, currency, tax behaviour, allowed payment providers
- [x] sales channel, and products belonging to some of them
- [x] publishable keys that pin a storefront to its channels

### 4. Catalogue

- [x] product, variant, option, option value
- [x] collection, category (nested), tag, type, images
- [x] slug unique within a scope; draft, published, archived
- [x] variant generation from options, and the combinations that are not sold
- [x] localisation: title and description per locale
- [x] listing: cursor pagination, allowlisted filters, a query-count test

### 5. Pricing

- [x] `price_set` per variant and per shipping option
- [x] price by currency, with quantity bands
- [x] price rules: region, customer group, channel, and custom attributes
- [x] resolution: most specific wins, ties by priority, a default underneath
- [x] price list: dated, conditional, sale or override, with its own rules
- [x] tax-inclusive pricing as a per-currency, per-region preference

### 6. Inventory and locations

- [x] `inventory_item`, `inventory_level`, `reservation_item`
- [x] stock locations, and which channels each serves
- [x] reserving raises `reserved` and leaves `stocked` alone
- [x] fulfilling drops the reservation and lowers `stocked`
- [x] reservations expire, on the host's clock — see `Jobs` in the README
- [x] backorder as an explicit allowance, never an accident
- [x] one variant over several items, for bundles
- [x] concurrency test: two carts, one last unit, exactly one wins

### 7. Cart

- [x] cart, line item, adjustment, tax line, shipping method
- [x] a line item snapshots title, sku and options, so a later edit cannot
      rewrite history
- [x] totals computed one way, in one place
- [x] a guest cart becomes a customer's on sign-in
- [x] carts expire, and expiry releases what they reserved

### 8. Payment

- [x] `payment_collection` → `session` → `payment` → `capture` / `refund`
- [x] authorising and capturing are separate acts with separate permission
- [x] `PaymentProvider` trait, and a fake that can fail on purpose
- [x] Stripe, and iyzico
- [x] account holders: a saved customer at the provider
- [x] inbound webhooks: signature checked, `(provider, event_id)` unique, so a
      replay is stored once and acted on once
- [x] the amount charged is checked against the order, and a mismatch becomes a
      recorded state rather than a log line

### 9. Order

- [x] order, item, shipping method, addresses, a transaction ledger
- [x] status as an enum with declared transitions and a check constraint
- [x] a total that is always the sum of its lines, held by a property test
- [x] versioned items, so an edited order keeps what it looked like before
- [x] `order_change` and `order_change_action`: one mechanism for edit, return,
      exchange and claim, each with request, approve, decline
- [x] returns: what came back, why, and what it is worth
- [x] exchanges: a return and an outbound order that settle together
- [x] claims: damaged or missing, replaced or refunded
- [x] refunds put stock back and give a promotion use back
- [x] drafts: an order built in the back office and sent to be paid

### 10. Fulfilment

- [x] fulfilment set, service zone, geo zone (country, province, postcode)
- [x] shipping option, its rules, flat and calculated pricing
- [x] fulfilment, shipment, tracking, labels
- [x] `FulfillmentProvider` trait; manual fulfilment first
- [x] partial fulfilment, and cancelling one

### 11. Tax

- [x] tax region, nested by parent, with rates and rules
- [x] one default rate per region, enforced by a unique index
- [x] combinable rates that stack
- [x] `TaxProvider` trait, for a host that calculates elsewhere
- [x] inclusive and exclusive, both correct at the line

### 12. Customer

- [x] customer, addresses, groups
- [x] a guest becoming a customer keeps their orders
- [x] erasure and export, so a host can answer a data request

### 13. Promotion

- [x] promotion, application method, campaign
- [x] fixed and percentage; across an order, per line, or on shipping
- [x] rules, target rules, buy rules
- [x] buy-X-get-Y
- [x] campaign budget, by spend or by count, decremented atomically
- [x] usage limits per shop and per customer, claimed at checkout rather than
      counted at payment
- [x] several promotions on one cart, applied in a defined order

### 14. Surfaces

- [x] store API: catalogue, cart, checkout, orders, customer, returns
- [x] admin API: everything else
- [ ] OpenAPI generated from the code, snapshotted, client types generated —
      generated and snapshotted (`tests/openapi.rs`). `src/api/openapi.rs`
      derives request and response schemas from the same `Serialize`/
      `Deserialize` Rust types the handlers already take and return
      (`schemars`), keeping `Page<T>` to one shared schema regardless of `T`
      rather than one copy per list. The payout domain proved the mechanism
      on a whole domain, request and response bodies both; 22 operations
      carry a schema now — payout's 6 plus, response side only, the list and
      single-fetch operation for every view type
      [`app/client/src/api/views.ts`](app/client/src/api/views.ts) hand-transcribes
      across the seven domains it actually reads (catalogue, order,
      inventory, customer, promotion, subscription, store). `views.ts` is not
      deleted yet — that is the next step once its schemas are checked
      against what `schemars` now generates for the same types — and the
      create/edit routes in those seven domains, which `app/client/` does not
      call yet, still answer `200` with no body schema, the same as the rest
      of the table. productdevbook/tezgah#202.
- [x] every route declares its permission, and a matrix test proves it —
      `tests/api_permissions.rs` calls 355 of the 446 routes in `routes()`
      against a host that denies everything and asserts `denied`; 91 are
      named in a reasoned, shrink-only `TOLERATED` list rather than called.
      2 need a `PaymentProvider`/`RecurringProvider` fixture this test does
      not build. The other 89 are a real ordering gap this matrix surfaced:
      a handler loads its row and only then asks permission, because the
      permission it must ask depends on an owner (`customer_id`) the row
      alone carries — so a synthetic id answers `not_found` instead of
      `denied`. 40 are `admin_order.rs`'s own return/exchange/claim helpers
      (productdevbook/tezgah#151); 49 are the crate's own core —
      `order::get` and the rest of `order.rs`'s `OrderId` functions,
      `order_basket::get`, `subscription::get` — the same shape one layer
      deeper (productdevbook/tezgah#152). Existing rows stay protected
      either way; only a nonexistent id gets far enough to distinguish
      "does not exist" from "exists but not yours". Neither is fixed here:
      the fix changes how a resource whose owner is discovered by loading
      it gets judged, a `ports.rs`-level decision, not a one-line one. The
      matrix already found and fixed one live bug this way before this
      session: `GET /admin/workflows-executions/{id}` answered a denying
      host with `not_found`.
- [x] listing, filtering and sorting consistent across every collection

### 15. Proof

- [x] a scope cannot see another's rows — a generated test per table —
      `tests/isolation.rs` reads the catalogue for its 143 registered tables
      rather than a list kept by hand, seeds one row per table through a
      real non-superuser role under forced row-level security, then asks a
      second scope's own connection to `select`, `update` and `delete` that
      row: all three must come back empty, and the run names the table if
      one does not. A companion test keeps the seeder honest —
      `NOT_YET_SEEDED` is empty and may only shrink, so a table the seeder
      cannot fill fails the run instead of quietly sitting outside the
      check. Measured against a fresh migration head, all 143 registered
      tables seed and all 143 come back empty from another scope; none
      needed the seeder taught anything new, and none leaked.
      `tests/schema.rs` carries the half this is not: two catalogue-wide
      checks that row-level security is forced and a policy exists at all
      (config, not behaviour), plus three hand-written cases against one
      table, `workflow_run` — an unset scope, a write refused into somebody
      else's — for shapes a generated seeder does not reach. A third
      generated sweep, `only_the_named_exception_crosses_a_scope`, checks
      every single-column foreign key that could name a row outside its own
      scope and finds none does except the two migrations register on
      purpose (`order.basket_id`, `cart.basket_id`) — and that even those
      grant a reference, not a read.
- [x] a customer cannot reach another customer's row on the storefront —
      `tests/api_store_cross_tenant.rs` seeds two shoppers, each with a cart,
      an order, an address, a payment collection, a subscription and a
      downloadable entitlement, and calls every `Surface::Store` route that
      names one of those by id twice: as the owner (must succeed) and as the
      other shopper (must refuse). 33 routes are exercised this way; 11 with
      no id to probe (`SELF_ONLY`) and 22 catalogue/config reads with no
      customer dimension (`PUBLIC`) are named rather than called, and 4 —
      `carts/{id}/complete` and the token-gated transfer and download routes
      — are `TOLERATED` for a stated reason, the transfer and download ones
      still exercised for "the wrong token refuses". Every real failure this
      run was the fixture's, not the crate's: a fabricated id where a real
      foreign key was expected aborted the transaction outright rather than
      answering an error worth asserting on, and the completeness check
      caught three routes never called at all. Nothing here found a new #82,
      #132 or #135 — this is the matrix that would have, and now stands
      watch for the next one. This is the customer axis; the channel axis
      (#132, #135) is `api_store.rs`'s and the scope axis is the bullet
      above.
- [ ] every state machine's illegal moves rejected, tested exhaustively
- [ ] money invariants hold under random operation sequences
- [x] a checkout interrupted at each step leaves no stock reserved, no money
      captured, no half-order — all eight steps of `Checkout::workflow()`, each
      with a test that fails it and asserts nothing is left behind. Two were
      missing and the box above this one said otherwise: `create_subscriptions`
      was only tested in the reverse direction, and `redeem_credit` — the step
      that spends a gift card or store credit — was not tested in either, so
      no test had ever spent a balance and checked it came back. Both are
      covered now, and the balance is asserted equal to what it was, not
      merely greater than zero.
- [ ] a webhook delivered twice, out of order, and late, is handled once
- [x] no listing endpoint can return an unbounded number of rows — `Paging`'s
      `limit` field is private and `Paging::limit()` is
      `unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)` with `MAX_LIMIT` at 200,
      so `limit=1000000` and no limit at all both end at 200. Every one of the
      67 paged queries binds `paging.probe()`, which is that number plus one.
      `tests/no_unbounded_list.rs` reads `src/` and fails when a public
      function hands back a `Vec` that is neither paged nor capped by a named
      constant; its `TOLERATED` list is empty.

### 16. The host half

Stages 2 to 15 are the commerce engine, and the sweep against an established
platform's commerce surface is done. What is not done is the layer around it:
the things that are nobody's problem while tezgah is only a library, and
become this repository's problem because `app/` ships a shop somebody else
runs. [`docs/architecture.md`](docs/architecture.md) carries each of these
with the measurement behind it and the layer that owns the fix; this is the
list.

In the library:

- [ ] search on orders and customers — the catalogue has one (`page::Search`,
      `ilike`, no index); both of the others take their filters as positional
      arguments rather than a struct, so it is a signature change
- [ ] sorting on the lists past the three that have it — products by title,
      orders and customers by address. The cursor carries a key, so each list
      left needs a column and a `page::By` variant rather than a design, and
      an exhaustive `match` makes a variant added without one a compile error
- [ ] the query string of the other 480 operations in the document — three
      describe theirs (#254); the rest still answer with their path
      parameters alone
- [ ] a count beside a page on every list. `Page<T>` carries `total`, and the
      three lists that filter — products, orders, customers — answer it when
      asked, each from a macro both its queries read so the count cannot drift
      from the page. The rest still say `null`
- [ ] something that *acts* on a payment provider's callback. There is a
      route now — `POST /webhooks/payments/{provider}` on its own surface,
      signed, mounted only with a secret, and a redelivery lands once — but
      it records and answers. Capturing or moving an order's state from what
      the provider said is provider-specific mapping, and
      `GET /admin/payment-webhooks` is where what has arrived waits
- [ ] the 89 routes that answer `not_found` where they mean `denied`, because
      the owner is only known once the row is loaded (#151, #152)

In `app/`:

- [ ] anything that needs a letter — an invitation, a notification, a reset
      link. Accounts, sessions, revocation and an owner-set password reset all
      exist without one; a link this server cannot send would be worse than
      one it never offered
- [ ] a storefront sign-in, and then per-row authorization. Three roles are
      checked at the door against the `Action` the route table declares, which
      answers "may this person refund anything". "May this person refund this
      order" is an `Authorizer`, and `Resource` already carries the owner on
      the five kinds that have one — but the app has no actor to compare
      against: its storefront runs as a guest whose cart id came from the same
      path parameter it is asked about, so a rule comparing the two refuses
      nothing. The sign-in comes first
- [ ] the rest of the letters. An event has somewhere to *go*, a file has
      somewhere to live — a directory, five image types, a name this binary
      chooses — and a mailer exists, which an operator invitation uses. What
      is left is a shopper's order confirmation and a password reset somebody
      can ask for themselves
- [ ] the rest of the route table: 160 of 486 bound by hand, 228 drawn by the
      panel. Counted from the other side: of the 77 `/admin/…/{id}/…`
      sub-routes, 70 were drawn by no screen and ten of those were already
      bound — the panel had simply never asked. Those ten are drawn now, and
      the other 60 need binding first
- [ ] tracing, metrics, a request log, readiness apart from liveness, a CORS
      policy, a rate limit

In the panel:

- [ ] filtering, searching and sorting, once the library offers them
- [x] the rest of the forms on the resolver — `react-hook-form` against the
      zod schema in fourteen files across five domains. What is left carries
      no fields worth validating: a confirmation with a reason, an upload
      button, and the two edit grids
- [ ] the screens' own words in the dictionary — English and Turkish exist
      and the compiler keeps them in step, over the shared chrome only
- [x] an edit grid past prices — an inventory item's levels are counted in
      one, for the same reason `/pricing/prices` can be one: the batch route
      takes the rows together. A list whose writes are one row at a time
      still cannot have one worth using, so the third is not free
- [x] the routing half of the seam — a host renders `<Panel basepath=…/>`
      and gets the whole tree under its own prefix. The router is built per
      mount rather than at module load, and `check:host` fails the build if
      anything outside the standalone host reads a browser global. What is
      left there is packaging: no library build, so a host takes the source
- [x] a test for a child route whose parent draws no outlet — all five "edit
      a record" screens were unreachable that way, silently, from the commit
      that added them. `scripts/check-outlets.mjs`, in CI

## Deliberately not built

**Under licence.** Medusa's RBAC and SSO paths need a commercial agreement as
of 11 August 2026. They were removed from the reference tree unread. tezgah
asks a host's `Authorizer` and has no role system of its own to compare.

**By design.** Three of Medusa's decisions are taken the other way, and
[README](README.md) says why: modules that may not reference each other and are
joined in application memory; amounts stored twice, as a numeric column beside
a JSON `raw_` one; a search index built to answer what cross-module joins
could not.

**Because a host has it.** Everything in the table at the top.

**Not yet, and named so it can be asked for.** Converting between currencies.
(Moving stock between warehouses as one atomic act was on this list; it is
built now — `inventory::transfer_stock`, routed at
`src/api/admin_catalogue.rs:2344`.)

Four things that were on this list have since been built and are no longer
absent: multi-seller marketplaces, subscriptions and recurring billing, gift
cards and store credit, and bundles priced as a unit.
