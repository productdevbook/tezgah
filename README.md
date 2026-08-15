# tezgah

A commerce engine as a Rust library — the shop behind the shop. Catalogue,
pricing, stock, carts, checkout, orders, payments, fulfilment, subscriptions
and a marketplace, with an HTTP surface for an admin panel and a storefront.

It is a library rather than a service. It owns tables in *your* Postgres and
runs in *your* transaction, so an order and whatever else that request wrote
commit together or not at all. No second database to restore, no sidecar to
keep alive, no HTTP hop between your handler and your stock.

**Status: early.** Nothing is stable, there is no release, and the version is
`0.0.0` on purpose.

## What it does

| Domain | What it covers |
|---|---|
| `catalogue` | products, variants, options, collections, nested categories, tags, images, per-locale content, variant generation from options |
| `pricing` | price sets per variant and shipping option, quantity bands, rules by region / customer group / channel, price lists (dated, sale or override), bundle pricing |
| `inventory` | items, levels, reservations, stock locations, lot and serial tracking with FEFO/FIFO, expiry, recall lookup, transfers between warehouses, backorder as an explicit allowance |
| `cart` | line items, bundles, adjustments, tax lines, shipping methods, guest carts that become a customer's on sign-in |
| `checkout` | the workflow: reserve, claim promotions, price, tax, create the order, take the money — and unwind all of it when a later step fails |
| `order` | versioned items, edits, returns, exchanges, claims, draft orders, transfers, invoices and credit notes, agreements and the withdrawal window |
| `payment` | collections, sessions, authorise / capture / refund, instalments and their surcharge, webhooks that land once |
| `settlement` | what a captured payment obligates the shop to, in one place |
| `credit` | gift cards and store credit, as a balance rather than a discount |
| `digital` | files sold, entitlements granted on payment, download tokens and counts |
| `subscription` | selling plans, contracts, renewals, dunning, pause / resume / skip / swap, prepaid periods, proration |
| `promotion` | rules, targets, campaigns and their budgets, buy-X-get-Y |
| `tax` | regions, rates, rules, registrations, exemptions, OSS/IOSS, per-line place of supply |
| `customer` | accounts, addresses, groups, export and erasure |
| `fulfilment` | providers, profiles, service and geo zones, shipping options, shipments, labels |
| `order_basket`, `payout` | a marketplace: one basket across sellers, one payment, a payout ledger and commission |
| `batch` | CSV import and export of products, prices and stock |
| `api` | both surfaces — admin and storefront — as one route table, with an OpenAPI document generated from it |

Every one of these is reachable from a route; that is checked by a test rather
than asserted here.

## Getting started

Postgres 15 or later, and a `sqlx` pool.

```toml
[dependencies]
tezgah = { git = "https://github.com/productdevbook/tezgah" }
```

**Migrations** ship with the crate:

```rust
tezgah::MIGRATIONS.run(&pool).await?;
```

**Implement the five ports.** Everything tezgah needs from you is in
[`src/ports.rs`](src/ports.rs). The smallest host that works — no authorization
of its own, nothing recorded anywhere — is this:

```rust
use tezgah::ports::{Action, Actor, Authorizer, Clock, Permit, Resource};

struct Bare;

impl Authorizer for Bare {
    fn authorize(&self, _: &Actor, _: Action, _: &Resource) -> tezgah::Result<Permit> {
        Ok(Permit::granted())
    }
}

impl Clock for Bare {
    fn now(&self) -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }
}
```

`AuditSink`, `EventSink` and `Jobs` take a transaction and may do nothing at
first. Implement all five and `Host` arrives on its own through a blanket impl.

**Assemble a `Ctx` once per request** and pass it down:

```rust
let ctx = Ctx::new(Scope(shop_id), Actor::Customer { id: customer_id }, &host);
```

`Scope` is which shop's data this is. A single-shop host uses one fixed value
and never thinks about it again.

**Then the domain functions take a transaction you opened:**

```rust
let mut tx = pool.begin().await?;
let line = tezgah::cart::add_line(&mut tx, &ctx, cart_id, add).await?;
tx.commit().await?;
```

**Checkout takes the pool instead**, because it is not one transaction — it
reserves stock, asks a provider for money and writes an order, and the provider
is not in your database:

```rust
let placed = Checkout::new(provider, location_id)
    .place(&pool, &ctx, cart_id, None)
    .await?;
```

If a later step fails, the runner walks back through the earlier ones and undoes
what they did.

## The HTTP surface

`src/api/` carries both surfaces as one route table — `api::routes()` — read by
the router, by the OpenAPI generator, and by the tests that check each route
asks the permission it declares. The generated document is committed at
[`tests/snapshots/openapi.json`](tests/snapshots/openapi.json), so a change to
the API is a change to a file somebody reviews.

Handlers are plain functions taking a pool or a transaction and a `Ctx`. There
is no framework here: bring axum, or actix, or whatever you already run.

## What it asks of you

tezgah decides nothing it does not have to. It asks, and believes the answer:

| Port | You supply | Why it is yours |
|---|---|---|
| `Authorizer` | who may do what | you already have roles; tezgah should not invent a second set |
| `AuditSink` | where a change is written down | written in the same transaction, so a rollback takes the audit row with it |
| `EventSink` | where a domain event goes | an outbox, not a publish — delivery is yours |
| `Jobs` | how deferred work is queued | enqueued in the same transaction as the change it belongs to |
| `Clock` | what time it is | so "expires in an hour" is testable without sleeping |

Some carry obligations that are easy to miss and expensive to get wrong — an
`Authorizer` that denies `Actor::System` silently stops every subscription
renewal in the shop. [`docs/hosting.md`](docs/hosting.md) is the list: what
`Jobs` is for and what it is not, which function to call when money arrives,
how a card reference is saved, and how a marketplace checkout is split across
sellers.

## How it holds together

**Every table carries a `scope`** and ships row-level security policies reading
it. A multi-tenant host sets `app.scope` on its transaction and Postgres
enforces the rest.

**Every public function that reaches the database asks your `Authorizer`
first.** The `Permit` it returns is the answer, not a token the compiler makes
each call carry: what is checked is that the question was put.

**The rules are enforced by tests that read the source or the catalogue**,
rather than by convention — that a function has a caller and a route, that a
route refuses a caller the host refuses, that a storefront route refuses another
shopper's row and serves its own, that anything moving money or destroying a row
leaves an audit record, that a migration's backfill announces its scope. Each
carries a list of exceptions that may only shrink.

## Decisions

**One Postgres, real foreign keys.** Medusa isolates its modules so completely
that they may not reference each other, and joins them in application memory
instead. The benefit is running a module against a separate database. Nobody
does. tezgah writes the join.

**Amounts are `NUMERIC`, not minor units and not floats.** Medusa stores every
amount twice — a numeric column to query and a JSON `raw_` column that is the
real one — because JavaScript numbers lose precision. Rust has `Decimal`. A
currency's exponent is a formatting fact, so nothing is multiplied by a hundred
on the way in.

**Modules split by domain, not by ceremony.** One crate, one file per domain. A
workspace split earns its keep when a second binary needs a subset, and not
before.

**The workflow runner is the point.** Each step declares how to undo itself.
State lives in `workflow_run` and `workflow_step`, claims use `FOR UPDATE SKIP
LOCKED`, and there is no Redis. Capture has no compensation on purpose:
captured money is not un-captured, it is refunded, which is its own step with
its own record.

**Payments belong to [kasapay](https://github.com/productdevbook/kasapay)** —
one payment API over any provider. What lives here is the mapping onto its
trait and what tezgah does with the answer.

## What is deliberately absent

Routing one fulfilment across several warehouses, and converting between
currencies. A shop with stock in two places picks the location; a shop selling
in two currencies prices in both.

Translating the interface a shopper reads, formatting a number for their
locale, and reporting over what already sits in Postgres — those belong to
whatever a host is built in, for the same reason tezgah ships no admin UI. A
product's own content is different: `catalogue` carries a title, a description
and a handle per locale, because that text is the shop's data rather than the
surrounding chrome.

Each is a real feature for somebody and none is needed to sell a thing. They
are absent because they were considered, not because they were forgotten.

## Design provenance

The data model is informed by Medusa's published design, read at v2.18.0 under
MIT; its commerce surface at v2.19.0 is the yardstick for what a shop should
not have to write by hand. No source, comment, test or fixture was copied. See
[NOTICE](NOTICE) for what that means and for the decisions taken the other way.

## Licence

MIT.
