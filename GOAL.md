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
| analytics dashboards, admin UI | the API | a Rust library does not ship React |

## Where this is

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
      generated and snapshotted (`tests/openapi.rs`); client-type generation
      unverified this session
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

- [ ] a scope cannot see another's rows — a generated test per table
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
- [ ] a checkout interrupted at each step leaves no stock reserved, no money
      captured, no half-order
- [ ] a webhook delivered twice, out of order, and late, is handled once
- [ ] no listing endpoint can return an unbounded number of rows

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
