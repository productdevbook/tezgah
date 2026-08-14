# tezgah

A commerce engine as a Rust library. Products, prices, inventory, carts,
orders, payments, fulfilment — and a workflow runner that unwinds what it
started when a later step fails.

It is a library rather than a service. It owns tables in *your* Postgres and
runs in *your* transaction, so an order and whatever else that request wrote
commit together or not at all. There is no second database to restore, no
sidecar to keep alive, and no HTTP hop between your handler and your stock.

**Status: early. Nothing here is stable and there is no release yet.**

## Why

The Rust ecosystem has no commerce engine. The highest-starred attempt is a
CMS with twenty-eight stars. So anyone selling something from Rust writes
`orders`, `stock` and a Stripe webhook by hand, and rediscovers in production
what everyone else already knows:

- Stock decremented at payment, not at checkout, oversells for the length of
  the payment redirect.
- A discount divided across lines and rounded per line stops matching the
  total the provider was asked to charge — so the money arrives and the order
  stays pending.
- A webhook arrives twice, and once more after the order was already fulfilled.
- Nothing rolls back the order that was created just before the payment failed.

None of these are hard problems. They are all *known* problems, and they are
the reason this exists.

## What it asks of you

tezgah decides nothing it does not have to. It asks, through traits in
[`src/ports.rs`](src/ports.rs), and believes the answer:

| Port | You supply | Why it is yours |
|---|---|---|
| `Authorizer` | who may do what | you already have roles; tezgah should not invent a second set |
| `AuditSink` | where a change is written down | written in the same transaction, so a rollback takes the audit row with it |
| `EventSink` | where a domain event goes | an outbox, not a publish — delivery is yours |
| `Jobs` | how deferred work is queued | same transaction; enqueue and the change it belongs to commit together |
| `Clock` | what time it is | so "expires in an hour" is testable without sleeping |

Two of those are load-bearing for anything that happens with nobody watching.
A renewal runs as `Actor::System`, because there is no shopper in a browser to
run it as: an `Authorizer` that denies `System` stops every subscription in the
shop from renewing, and stops them silently, since the only caller is a cron.
`Jobs` is where a declined renewal's next attempt is queued, in the same
transaction as the `past_due` it belongs to — a host that implements it as a
no-op has a shop that stops charging somebody and never retries.

You assemble a `Ctx` once per request and pass it down. A host with none of
this uses `Permit::granted()` and a clock, and everything works.

Every public function that reaches the database asks your `Authorizer` before
it does, and `tests/permit_asked.rs` reads the crate's own source to keep that
true — a new function that queries without asking fails CI. The `Permit` an
authorizer returns is the answer, not a token the compiler makes each call
carry: what is checked is that the question was put, not threaded.

Every table carries a `scope` — one shop, one tenant, one marketplace seller —
and ships row-level security policies reading it. A single-shop host uses one
fixed scope and never thinks about it again. A multi-tenant host sets
`app.scope` on its transaction and Postgres enforces the rest.

**The entry point for money arriving is `settlement::capture`.** A route calls
it, and so does a host's own webhook handler — never `payment::capture_only`
directly, which takes the money and nothing more. `settlement::capture` calls
that and then everything a captured payment obligates the shop to: a
purchased gift card printed, a digital entitlement granted, a subscription's
first period started. `settlement::refund` is its mirror.

## Decisions

**One Postgres, real foreign keys.** Medusa isolates its modules so completely
that they may not reference each other, and joins them in application memory
instead. Its own code notes that a filter it cannot push down makes it fetch
the whole root set and paginate in Node, and it grew a second, denormalised
search engine to work around that. The benefit is running a module against a
separate database. Nobody does. tezgah writes the join.

**Amounts are `NUMERIC`, not minor units and not floats.** Medusa stores every
amount twice — a numeric column to query and a JSON `raw_` column that is the
real one — because JavaScript numbers lose precision. Rust has `Decimal`.
A currency's exponent is a formatting fact, so `Money` carries an amount and a
currency and nothing is multiplied by a hundred on the way in.

**Modules split by domain, not by ceremony.** One crate, `src/orders/`,
`src/inventory/`. A workspace split earns its keep when a second binary needs a
subset, and not before.

**The workflow runner is the point.** Checkout is not one transaction — it
reserves stock, asks a provider for money, writes an order, opens a fulfilment,
and the provider is not in your database. Each step declares how to undo
itself; when a later step fails the runner walks back through the earlier ones.
State lives in one `workflow_execution` table, claims use `FOR UPDATE SKIP
LOCKED`, and there is no Redis.

Capture has no compensation on purpose. Captured money is not un-captured; it
is refunded, which is its own step with its own record.

**What is deliberately absent:** multi-warehouse routing, geo-zoned shipping
rate tables, buy-X-get-Y promotions, campaign budgets, order-item version
history. Each is a real feature for somebody and none is needed to sell a
thing. They are absent because they were considered, not because they were
forgotten.

## Design provenance

The data model is informed by Medusa's published design, read at v2.18.0 under
MIT. No source, comment, test or fixture was copied; see [NOTICE](NOTICE) for
what that means and for the three decisions taken the other way.

## Licence

MIT.
