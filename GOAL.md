# Goal

A shop can be built on tezgah without dropping to raw SQL for anything a shop
normally does. Concretely: a catalogue with variants and options, prices that
depend on currency and quantity, stock that is reserved rather than guessed,
a cart that survives its shopper signing in, a checkout that either finishes or
leaves nothing behind, payments that authorise and capture separately, orders
that can be returned, exchanged and edited, fulfilment that produces a shipment,
tax that is right for the region, and promotions that come off the right lines.

Both API surfaces — the shopper's and the back office's — are complete and
described by an OpenAPI document generated from the code.

**Not in scope, and deliberately:** multi-warehouse routing, geo-zoned shipping
rate tables, buy-X-get-Y promotions, campaign budgets, order-item version
history, a marketplace with several sellers per order. Each is real for
somebody; none is needed to sell a thing.

## Order of work

Nothing below starts before the thing above it works, because each is the
foundation the next one stands on.

### 1. Foundation

- [x] `ports` — what a host supplies: authorization, audit, events, jobs, clock
- [x] `Money` — `Decimal` and a currency; `allocate` whose parts add back up
- [x] `Error` — one canonical struct, private cause, stable `code()`
- [ ] `scope` — the column, the RLS policies, the schema test that no table escapes
- [ ] `Id` — typed ids (`ProductId`, `OrderId`) so two uuids cannot be swapped
- [ ] test harness — a real Postgres per test, two scopes seeded, run in parallel

### 2. The workflow runner

The reason this is a library and not a pile of tables. Checkout is not one
transaction: it reserves stock, asks a provider for money, writes an order,
and the provider is not in the database.

- [ ] `Step` — invoke, and how to undo what invoking did
- [ ] compensation — a later failure walks back through the earlier steps
- [ ] `workflow_execution` — one table, checkpoints, claims by `FOR UPDATE SKIP LOCKED`
- [ ] idempotency — the same transaction id twice resumes rather than repeats
- [ ] retry with backoff, a ceiling, and a dead letter
- [ ] timeouts, and a lease a long step extends while it runs
- [ ] tests: kill a worker mid-flow and watch it unwind; run two, watch one win

### 3. Catalogue

- [ ] product, variant, option, option value
- [ ] collection, category, tag, image
- [ ] slug uniqueness within a scope; publish state
- [ ] listing: cursor pagination, filters from an allowlist, a query-count test

### 4. Pricing

- [ ] `price_set` per variant and per shipping option
- [ ] price by currency, with quantity bands
- [ ] price list: a dated, conditional overlay (sale and override)
- [ ] resolution: most specific match wins, ties broken by priority
- [ ] tax-inclusive pricing as a per-currency preference

### 5. Inventory

- [ ] `inventory_item`, `inventory_level`, `reservation_item`
- [ ] reserving raises `reserved` and does not touch `stocked`
- [ ] fulfilling drops the reservation and lowers `stocked`
- [ ] reservations expire, and expiry is a job
- [ ] backorder as an explicit allowance, never an accident
- [ ] concurrency test: two carts, one last unit, exactly one wins

### 6. Cart

- [ ] cart, line item, adjustment, tax line, shipping method
- [ ] a line item snapshots title, sku and options, so a later edit cannot rewrite history
- [ ] totals: subtotal, discount, shipping, tax, total — computed one way, in one place
- [ ] a guest cart becomes a customer's on sign-in
- [ ] carts expire, and expiry releases what they reserved

### 7. Payment

- [ ] `payment_collection` → `session` → `payment` → `capture` / `refund`
- [ ] authorise and capture are separate acts with separate permission
- [ ] `PaymentProvider` trait; a fake for tests
- [ ] Stripe and iyzico behind it
- [ ] inbound webhooks: signature verified, and `(provider, event_id)` unique so a
      replay is stored once and acted on once
- [ ] the amount charged is checked against the order, and a mismatch is a
      recorded state rather than a log line

### 8. Order

- [ ] order, line item, shipping method, address, transaction ledger
- [ ] status as an enum with declared transitions, and a check constraint
- [ ] a total that is always the sum of its lines, asserted by a property test
- [ ] `order_change` and `order_change_action`: one mechanism for return,
      exchange and edit, with request, approve and decline
- [ ] refunds that put stock back and give a coupon use back

### 9. Fulfilment

- [ ] shipping option and its rules
- [ ] fulfilment, shipment, tracking
- [ ] `FulfillmentProvider` trait; manual fulfilment as the first implementation
- [ ] partial fulfilment, and cancelling one

### 10. Tax

- [ ] tax region, rate, and rules
- [ ] one default rate per region, enforced by a unique index
- [ ] inclusive and exclusive pricing, both correct at the line

### 11. Customer

- [ ] customer, address, group
- [ ] a guest becoming a customer keeps their orders
- [ ] erasure and export, so a host can answer a data request

### 12. Promotion

- [ ] promotion, application method, rules
- [ ] fixed and percentage, across an order or per line
- [ ] usage limits, per shop and per customer, claimed atomically at checkout
      rather than counted at payment

### 13. Surfaces

- [ ] store API: catalogue, cart, checkout, orders, customer
- [ ] admin API: everything else
- [ ] OpenAPI generated from the code, snapshotted, and a client type generated from it
- [ ] every route declares its permission, and a matrix test proves it

### 14. Proof

- [ ] a scope cannot see another's rows, tested per table by a generated test
- [ ] every state machine's illegal moves are rejected, tested exhaustively
- [ ] money invariants hold under random operation sequences
- [ ] a checkout interrupted at each step in turn leaves no stock reserved,
      no money captured and no half-order
- [ ] a webhook delivered twice, out of order, and late, is handled once

## How it is done

Read `CLAUDE.md`. The rules that catch people: nothing reaches data without a
`Permit`; audit, events and jobs are written in the caller's transaction, never
after it; every table carries a scope and forces row-level security; tests run
against a real Postgres, and concurrency claims are tested concurrently.
