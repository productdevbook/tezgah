# Measured against Medusa

The question this file answers is "what is missing, compared to the commerce
platform most people have already seen". It is a list of gaps with an owner
against each, not a scorecard, and the two halves were measured rather than
remembered: tezgah's numbers come from counting its own code — the route
table, the query types in `src/api`, the traits in `src/ports.rs` — and
Medusa's from its published documentation, read on 2026-08-21. Anything
either project changes after that date makes a line here stale, and a stale
line is a bug in this file.

## The two are shaped differently on purpose

Medusa is a set of modules resolved at runtime through a container. A
deployment picks a provider for each infrastructure concern and can add
modules of its own. tezgah is one Rust crate over one Postgres: what a host
supplies is five traits in `src/ports.rs` — `Authorizer`, `Clock`,
`AuditSink`, `EventSink`, `Jobs` — and what a payment, tax or carrier
integration supplies is four more.

That is a decision rather than a gap. One database and one transaction is
what lets a workflow compensate everything it started when a later step
fails, and lets `scope` plus forced row-level security be a property of the
database rather than of the code that queries it. Nothing below proposes
trading it away.

## Domains

Medusa ships 22 commerce modules. tezgah covers most of what they hold, but
not as separate modules — a currency, a region, a sales channel and a stock
location live inside `store.rs` and `catalogue.rs` rather than in files of
their own, and each has routes: 3, 7, 6 and 9 of them respectively, plus 7
for publishable API keys.

Three of Medusa's have no counterpart here:

| Medusa module | tezgah |
|---|---|
| Auth | none, deliberately — the `Authorizer` port asks a host, and `app/server` answers with operator accounts, sessions and invitations of its own |
| User | none, same reason: tezgah does not know who its callers are |
| Loyalty | none. Store credit and promotions cover part of what it is used for; points, tiers and earning rules are absent |

Translation is partial rather than absent: four tables carry it — products,
categories, shipping options, return reasons — where Medusa has a module for
it.

Going the other way, tezgah carries several things Medusa's core does not:
subscriptions and selling plans, marketplace payouts with commission rules and
settlement, order baskets that span sellers, digital entitlements, inventory
lots with expiry and recall, and a workflow runner whose steps and
compensations are rows in the same database as the order they are unwinding.

## The gaps that matter

**1. A list cannot be asked a question.** 29 query types in `src/api` take a
cursor. Four of them carry a text search and a choice of ordering —
`ListProducts`, `ListOrders` and `ListCustomers` on the admin surface, and the
storefront's own `ListProducts`. Thirteen carry one or two narrowing fields,
usually the id of the row they hang off. The other twelve are a cursor and a
limit and nothing else, and 78 queries in the crate end `order by created_at`.
Medusa's admin list endpoints take `limit` and `offset`, an `order` that any
field can be named in, filters with operators like `$lt` and `$gt`, and answer
with a `count`.

This is the widest gap and the one everything else waits on: a back office is
mostly lists, and a list that cannot be narrowed is a list somebody scrolls.
It belongs to the library — a filter type per domain and one shared ordering,
built as SQL predicates the database can index, not a query language over
HTTP.

**2. No field selection.** Every view is fixed: what `ProductView` carries is
what a caller gets. Medusa's `fields` picks columns and relations, with `+`,
`-` and `*relation`. Library, and tangled with (1) — the same query builder
answers both.

**3. Paging is cursor-only.** `Paging` carries `after`, `limit` and whether to
count. There is no offset, so nothing can draw "page 7 of 20" or jump. That is
a real trade — a cursor is stable while rows are being written and an offset is
not — but it is a trade the documentation should state rather than one a panel
discovers.

**4. Nothing is chosen by configuration.** Medusa ships providers for caching
(Redis, Memcached), events (Local, Redis), files (Local, S3), locking (Redis,
Postgres), notification (Local, SendGrid), analytics (Local, PostHog) and the
workflow engine (In-Memory, Redis), each swapped in a config file. The shipped
tezgah binary has one demo payment provider that takes no money, a local
directory for files, an SMTP mailer and an outbox that posts to one URL. It
wires no tax provider and no carrier at all, though the library has traits for
both — measured: zero uses of `TaxProvider` or `FulfillmentProvider` in
`app/server`.

Mostly the app's gap rather than the library's. The library's part is small
and specific: the traits exist, the binary does not offer a way to say which
implementation to use.

**5. No notification port.** The library has no notion of a message to a
person; the app sends two plain-text letters directly. A shopper gets no order
confirmation from either. Medusa has a module with providers and templates.
This is the one place a sixth port is arguable — and the alternative, an event
a host subscribes to, is already there, which is the argument against.

**6. No file port.** A product image is a URL in a column and the app writes
files to a directory. Moving to object storage is a bucket behind the same
path rather than an interface. Deliberate for now; it stops being deliberate
the moment a second thing needs to store a file.

**7. The panel has no extension points.** Medusa's dashboard takes widgets
into named injection zones and whole pages from `src/admin/routes`, each able
to put itself in the sidebar. tezgah's panel is mountable — `<Panel/>` behind
`PanelProvider`, taking its API base, token, locale and basepath from a
runtime — but a host that mounts it cannot add a screen to it, or a card to a
screen it already has. For a host embedding the panel beside its own
application, that is the difference between adding commerce to a back office
and running two.

**8. One job kind cannot run, and it is the one the crate enqueues.** The
sweeps and the queue both work: `app/server/src/schedule.rs` calls
`cart::expire` and `inventory::expire_reservations` every five minutes, and
the worker claims a job, dispatches by kind, retries with a doubling backoff
and leaves a dead row with its reason. What it cannot dispatch is a
subscription's dunning retry, which needs a provider that can charge a card
left on file — no published version of the payment library can name which
card (productdevbook/kasapay#225). The job records that as its reason and
waits, which is the right behaviour and still a shop that cannot retry a
declined renewal.

**9. Search is three `ilike` queries.** Products, orders and customers can be
searched by a substring. Nothing else can, and there is no index behind it.

## What is not going to be copied

- **A module container.** Runtime resolution buys plugging in and costs the
  single transaction. tezgah keeps the transaction.
- **A query language on the wire.** `$lt`, `$gt` and nested `fields` push
  query planning into the URL. Every filter here has to be a predicate the
  database can index, and that is easier to keep true when the filter is a
  typed struct.
- **A second payment abstraction.** Providers belong to kasapay; what belongs
  here is the mapping and what tezgah does with the answer.

## The order to do them in

For the self-hosted product, (4) comes first: an image whose only payment
provider takes no money is a demonstration, not a shop, and a real one is a
release of the payment library away rather than work here. Then (1) and (2),
which are one piece of work and the largest — a back office is mostly lists.
(7) is what decides whether a host embeds the panel or replaces it. (5), (6),
(8) and (9) are all worth doing and none of them stops a shop trading today.

`architecture.md` carries the same gaps against the layer that owns each, and
is the file to read before deciding which side of the seam a change belongs
on.
