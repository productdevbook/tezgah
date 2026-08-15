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

The Rust ecosystem has no commerce engine. So anyone selling something from
Rust writes `orders`, `stock` and a Stripe webhook by hand, and rediscovers in
production what everyone else already knows:

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
| `Jobs` | how deferred work is queued | enqueued in the same transaction as the change it belongs to |
| `Clock` | what time it is | so "expires in an hour" is testable without sleeping |

You assemble a `Ctx` once per request and pass it down. A host with none of
this uses `Permit::granted()` and a clock, and everything works.

Some of these have obligations that are easy to meet by accident and expensive
to miss — an `Authorizer` that denies `Actor::System` silently stops every
subscription renewal in the shop. [`docs/hosting.md`](docs/hosting.md) is the
list: what `Jobs` is for and what it is not, which function to call when money
arrives, how a card reference is saved, and how a marketplace checkout is split
across sellers.

## How it holds together

**Every table carries a `scope`** — one shop, one tenant, one marketplace
seller — and ships row-level security policies reading it. A single-shop host
uses one fixed scope and never thinks about it again. A multi-tenant host sets
`app.scope` on its transaction and Postgres enforces the rest.

**Every public function that reaches the database asks your `Authorizer`
first.** The `Permit` it returns is the answer, not a token the compiler makes
each call carry: what is checked is that the question was put, not that it was
threaded. `tests/permit_asked.rs` reads this crate's own source to keep that
true, and a function that queries without asking fails CI.

**Several other rules are enforced the same way** — by tests that read the
source or the database catalogue rather than a list somebody maintains. That a
route refuses a caller the host refuses. That a storefront route refuses
another shopper's row and serves its own. That anything moving money or
destroying a row leaves an audit record. That a migration's backfill announces
its scope before it runs. Each carries a list of exceptions that may only
shrink.

## Decisions

**One Postgres, real foreign keys.** Medusa isolates its modules so completely
that they may not reference each other, and joins them in application memory
instead. Its own code notes that a filter it cannot push down makes it fetch
the whole root set and paginate in Node, and it grew a second, denormalised
search engine to work around that. The benefit is running a module against a
separate database. Nobody does. tezgah writes the join.

**Amounts are `NUMERIC`, not minor units and not floats.** Medusa stores every
amount twice — a numeric column to query and a JSON `raw_` column that is the
real one — because JavaScript numbers lose precision. Rust has `Decimal`. A
currency's exponent is a formatting fact, so `Money` carries an amount and a
currency, and nothing is multiplied by a hundred on the way in.

**Modules split by domain, not by ceremony.** One crate, one file per domain —
`src/order.rs`, `src/inventory.rs`. A workspace split earns its keep when a
second binary needs a subset, and not before.

**The workflow runner is the point.** Checkout is not one transaction: it
reserves stock, asks a provider for money, writes an order, opens a fulfilment,
and the provider is not in your database. Each step declares how to undo
itself, and when a later step fails the runner walks back through the earlier
ones. State lives in `workflow_run` and `workflow_step`, claims use `FOR UPDATE
SKIP LOCKED`, and there is no Redis.

Capture has no compensation on purpose. Captured money is not un-captured; it
is refunded, which is its own step with its own record.

**Payments belong to [kasapay](https://github.com/productdevbook/kasapay).**
One payment API over any provider. What lives here is the mapping onto its
trait and what tezgah does with the answer — the collection, the ledger, and
the webhook table that makes a redelivery land once.

## What is deliberately absent

Routing one fulfilment across several warehouses, and converting between
currencies. A shop with stock in two places picks the location; a shop selling
in two currencies prices in both rather than turning one rate into another.

Translating the interface a shopper reads, formatting a number for their
locale, and reporting over what already sits in Postgres. Those belong to
whatever a host is built in, for the same reason tezgah does not ship an admin
UI. A product's own content is different: `catalogue` carries a title, a
description and a handle per locale, because that text is the shop's data
rather than the surrounding chrome.

Each of these is a real feature for somebody, and none is needed to sell a
thing. They are absent because they were considered, not because they were
forgotten.

## Design provenance

The data model is informed by Medusa's published design, read at v2.18.0 under
MIT; its commerce surface at v2.19.0 is the yardstick for what a shop should
not have to write by hand. No source, comment, test or fixture was copied. See
[NOTICE](NOTICE) for what that means, and for the three decisions taken the
other way.

## Licence

MIT.
