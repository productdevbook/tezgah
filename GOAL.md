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

229 tests green. Every domain in stages 2 to 13 has a module, a schema and
tests; both API surfaces are served, 295 routes, with an OpenAPI document
generated from the route table and snapshotted. Forty-seven of the issues
opened against this file are closed.

What is left is written as issues rather than as boxes here: the proof suite
(#48, #49), the payment providers moving onto kasapay (#53), the seventeen
listings that still want a page (#52), order transfer (#54), and six tables the
isolation seeder cannot yet build a row for (#55).

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

- [ ] `Step` — invoke, and how to undo what invoking did
- [ ] compensation — a later failure walks back through the earlier steps
- [ ] `workflow_execution` — checkpoints, claimed by `FOR UPDATE SKIP LOCKED`
- [ ] idempotency — the same transaction id twice resumes rather than repeats
- [ ] retry with backoff, a ceiling, a dead letter
- [ ] timeouts, and a lease a long step extends while it runs
- [ ] advisory locks, so two checkouts on one cart do not interleave
- [ ] nested workflows, and steps that run in parallel
- [ ] tests: interrupt at every step in turn, assert nothing is left behind

### 3. Store, currency, region, sales channel

- [ ] store, its default currency and its supported ones
- [ ] currency, with the exponent used for rounding and display
- [ ] region: countries, currency, tax behaviour, allowed payment providers
- [ ] sales channel, and products belonging to some of them
- [ ] publishable keys that pin a storefront to its channels

### 4. Catalogue

- [ ] product, variant, option, option value
- [ ] collection, category (nested), tag, type, images
- [ ] slug unique within a scope; draft, published, archived
- [ ] variant generation from options, and the combinations that are not sold
- [ ] localisation: title and description per locale
- [ ] listing: cursor pagination, allowlisted filters, a query-count test

### 5. Pricing

- [ ] `price_set` per variant and per shipping option
- [ ] price by currency, with quantity bands
- [ ] price rules: region, customer group, channel, and custom attributes
- [ ] resolution: most specific wins, ties by priority, a default underneath
- [ ] price list: dated, conditional, sale or override, with its own rules
- [ ] tax-inclusive pricing as a per-currency, per-region preference

### 6. Inventory and locations

- [ ] `inventory_item`, `inventory_level`, `reservation_item`
- [ ] stock locations, and which channels each serves
- [ ] reserving raises `reserved` and leaves `stocked` alone
- [ ] fulfilling drops the reservation and lowers `stocked`
- [ ] reservations expire, on the host's clock — see `Jobs` in the README
- [ ] backorder as an explicit allowance, never an accident
- [ ] one variant over several items, for bundles
- [ ] concurrency test: two carts, one last unit, exactly one wins

### 7. Cart

- [ ] cart, line item, adjustment, tax line, shipping method
- [ ] a line item snapshots title, sku and options, so a later edit cannot
      rewrite history
- [ ] totals computed one way, in one place
- [ ] a guest cart becomes a customer's on sign-in
- [ ] carts expire, and expiry releases what they reserved

### 8. Payment

- [ ] `payment_collection` → `session` → `payment` → `capture` / `refund`
- [ ] authorising and capturing are separate acts with separate permission
- [ ] `PaymentProvider` trait, and a fake that can fail on purpose
- [ ] Stripe, and iyzico
- [ ] account holders: a saved customer at the provider
- [ ] inbound webhooks: signature checked, `(provider, event_id)` unique, so a
      replay is stored once and acted on once
- [ ] the amount charged is checked against the order, and a mismatch becomes a
      recorded state rather than a log line

### 9. Order

- [ ] order, item, shipping method, addresses, a transaction ledger
- [ ] status as an enum with declared transitions and a check constraint
- [ ] a total that is always the sum of its lines, held by a property test
- [ ] versioned items, so an edited order keeps what it looked like before
- [ ] `order_change` and `order_change_action`: one mechanism for edit, return,
      exchange and claim, each with request, approve, decline
- [ ] returns: what came back, why, and what it is worth
- [ ] exchanges: a return and an outbound order that settle together
- [ ] claims: damaged or missing, replaced or refunded
- [ ] refunds put stock back and give a promotion use back
- [ ] drafts: an order built in the back office and sent to be paid

### 10. Fulfilment

- [ ] fulfilment set, service zone, geo zone (country, province, postcode)
- [ ] shipping option, its rules, flat and calculated pricing
- [ ] fulfilment, shipment, tracking, labels
- [ ] `FulfillmentProvider` trait; manual fulfilment first
- [ ] partial fulfilment, and cancelling one

### 11. Tax

- [ ] tax region, nested by parent, with rates and rules
- [ ] one default rate per region, enforced by a unique index
- [ ] combinable rates that stack
- [ ] `TaxProvider` trait, for a host that calculates elsewhere
- [ ] inclusive and exclusive, both correct at the line

### 12. Customer

- [ ] customer, addresses, groups
- [ ] a guest becoming a customer keeps their orders
- [ ] erasure and export, so a host can answer a data request

### 13. Promotion

- [ ] promotion, application method, campaign
- [ ] fixed and percentage; across an order, per line, or on shipping
- [ ] rules, target rules, buy rules
- [ ] buy-X-get-Y
- [ ] campaign budget, by spend or by count, decremented atomically
- [ ] usage limits per shop and per customer, claimed at checkout rather than
      counted at payment
- [ ] several promotions on one cart, applied in a defined order

### 14. Surfaces

- [ ] store API: catalogue, cart, checkout, orders, customer, returns
- [ ] admin API: everything else
- [ ] OpenAPI generated from the code, snapshotted, client types generated
- [ ] every route declares its permission, and a matrix test proves it
- [ ] listing, filtering and sorting consistent across every collection

### 15. Proof

- [ ] a scope cannot see another's rows — a generated test per table
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

**Not yet, and named so it can be asked for.** Moving stock between warehouses
as one atomic act rather than two adjustments. Converting between currencies.

Four things that were on this list have since been built and are no longer
absent: multi-seller marketplaces, subscriptions and recurring billing, gift
cards and store credit, and bundles priced as a unit.
