# tezgah-server

A binary. `tezgah` (the crate one directory up) is a Rust library — it
decides nothing about how it is run, and owns no `main`. Somebody self-hosting
it needs a container that starts, reads its configuration from the
environment, opens a connection pool, runs migrations, and answers HTTP. This
is that container, in the same way `mavi-operator` is the hosting half over
`mavi`: a library plus the smallest honest thing that turns it into a service.

It is not the only way to run tezgah, and it does not try to be the complete
one. `../../examples/shop` is the other end of the same idea, kept deliberately
small: no router, no listener, five `tezgah::api::store` functions called
directly from a `main` that seeds one shop and walks catalogue → cart →
checkout → order as plain library calls. Read that file to see what embedding
tezgah looks like at its smallest. Read this one to see it made into
something a container starts.

`src/lib.rs` exists only so `tests/` can build the router and run `seed::run`
in-process — nothing outside this crate depends on it, and `main.rs` is still
the only thing that decides how this binary runs.

## Running it

```sh
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/tezgah
export ADMIN_TOKEN=$(openssl rand -hex 32)
export TEZGAH_STOCK_LOCATION_ID=<a stock location's uuid>
export TEZGAH_DEMO_BANK=i-understand-this-takes-no-money
cargo run --package tezgah-server
```

`DATABASE_URL` is the only setting this binary refuses to start without —
everything else has an honest default or an honest absence. It is read once,
at startup: a missing or empty value fails immediately, with one message
naming what is wrong, rather than on the first request that needed a pool.

| Variable | Required | Default | What it does |
|---|---|---|---|
| `DATABASE_URL` | yes | — | a `postgres://` connection string; startup fails clearly without one |
| `PORT` | no | `8080` | what this binary listens on |
| `TEZGAH_SKIP_MIGRATIONS` | no | unset | set to `1` to skip `tezgah::MIGRATIONS` — the database already has them |
| `ADMIN_TOKEN` | no | unset | the shared secret that makes the first operator account and gets back in when a password is lost — see "Who may reach the back office" |
| `TEZGAH_STOCK_LOCATION_ID` | no | unset | the one warehouse checkout reserves and ships from — see below |
| `TEZGAH_DEMO_BANK` | no | unset | set to exactly `i-understand-this-takes-no-money` to run checkout against the demo payment provider — see below |
| `TEZGAH_CURRENCY_EXPONENT` | no | `2` | this shop's one currency's decimal places, for the payment provider wrapper |
| `TEZGAH_EVENT_WEBHOOK` | no | unset | where an outbox row is posted; unset leaves every event written and unsent — see "Events leave the building" |
| `TEZGAH_EVENT_SECRET` | with the above | unset | signs the body. Startup fails if a webhook is set without one |
| `TEZGAH_PAYMENT_WEBHOOK_SECRET` | no | unset | the secret a payment provider's callback is signed with; unset leaves that route unmounted — see "A provider calls back" |

Configuration comes from the environment and nowhere else: no config file
format, because a container is not handed one separately from the
environment it was started with.

## Seeding a shop

`docker compose up` starts an empty shop: no currency, no region, no sales
channel, no stock location, no publishable key, and so nothing the storefront
routes or the panel's screens have to show. `tezgah-server seed` — the same
binary, one argument — writes the smallest shop a storefront can check out
from:

```sh
docker compose exec tezgah-server tezgah-server seed
# or, running the binary directly:
cargo run --package tezgah-server -- seed
```

It prints the stock location's id and a fresh publishable key, because both
belong in the environment next — the id as `TEZGAH_STOCK_LOCATION_ID`, so
`POST /store/carts/{id}/complete` gets bound on the next start; the key as
whatever a storefront sends in `x-publishable-key`. The key is shown once, the
same as every other publishable key `POST /admin/publishable-api-keys`
issues, and is not stored anywhere it could be read back — losing it means
issuing another.

Safe to run twice: `seed.rs`'s own doc comment says why a second run writes
nothing rather than a second shop. It seeds no product — `POST
/admin/products` and the write routes below are how a real catalogue goes in,
by hand or by whatever the panel or a script does with them.

### The one shop

tezgah's own README says a single-shop host sets `Scope` once and never
thinks about it again. This binary makes that literal: on every boot it reads
the first row of `tezgah_scope`, or creates one if the table is empty, and
runs as that shop for as long as it is up. There is no multi-tenant mode here
— a host that wants one runs several of these, or writes its own binary that
sets `app.scope` per request the way tezgah's row-level security expects.

### Checkout needs a warehouse

`tezgah::checkout::Checkout::new` takes a `StockLocationId` at construction —
tezgah's own README lists routing one fulfilment across several warehouses as
deliberately absent, so checkout is pinned to exactly one. Nothing bound here
creates a stock location, so `TEZGAH_STOCK_LOCATION_ID` has to name one that
already exists. Without it, `POST /store/carts/{id}/complete` is not bound at
all — see the route table below — rather than bound and answering every call
with a configuration error.

Checkout also needs a payment provider, and ships with a stand-in only:
`src/provider.rs`'s `DemoBank` authorises every charge immediately and
remembers nothing. `CLAUDE.md` is explicit that a provider is
[kasapay](https://github.com/productdevbook/kasapay)'s to write, not
tezgah's, and no adapter crate for a real bank or gateway lives in this
public repository — taking real money means depending on one and passing it
to `KasapayProvider::new` in `src/main.rs` in place of `DemoBank`.

Because `DemoBank` is the only provider this binary can build checkout with,
and it takes no real money, `TEZGAH_STOCK_LOCATION_ID` alone does not turn
checkout on. `TEZGAH_DEMO_BANK` also has to be set, to exactly
`i-understand-this-takes-no-money` — anything else, including unset or
empty, leaves `POST /store/carts/{id}/complete` unbound, and startup says
which of the two is missing. See
[`docs/self-hosting.md`](../docs/self-hosting.md#taking-real-money) for what
taking real money instead requires.

## `GET /docs` and `GET /openapi.json`

The API's own description, and [Scalar](https://scalar.com) reading it.
`/openapi.json` is what `tezgah::api::openapi::document()` generates from the
route table — the same document `tests/snapshots/openapi.json` pins, so what a
running server describes and what CI reviews cannot drift apart.

Both are open. The document says which paths exist and what permission each
asks, and every one of those paths already refuses an unauthorised caller on
its own; a description that needed protecting would mean the protection *was*
the description.

Neither counts against the 486: they describe them.

**It is thinner than it looks.** The document declares every operation and, for
most of them, no request or response body at all — `productdevbook/tezgah#202`
is that gap, and Scalar renders it honestly, which is part of why it is worth
serving.

## `GET /health`

Not "the process is running" — a probe can already tell that from the socket
accepting a connection — but "a query against Postgres still answers". Bound
unconditionally, unauthenticated, and not one of tezgah's own 486 declared
routes: it belongs to this binary, not to the crate.

## Who may reach the back office

tezgah authenticates nobody — `tezgah::ports::Authorizer` is a question a
host answers, and this binary's own `ServerHost` answers it by granting every
actor, `Actor::System` included, because `docs/hosting.md` says denying that
silently stops every subscription renewal. That is right for a library and
leaves the product to answer the rest.

**Operators.** `src/identity.rs` holds accounts with names and argon2id
passwords, and sessions that expire after thirty days. A session dies when
the account is disabled and when its password changes, so revoking somebody
is one request rather than a rotation everybody else has to be told about.
The tables are this binary's — no `scope`, no row-level security — for the
same reason `server_job` is: a person who runs the shop is not one of the
shop's rows.

**`ADMIN_TOKEN`.** Still here, still one shared secret checked in constant
time. It is how the first account is made, and how a shop that lost every
password gets back in. It is not a person, and nothing pretends otherwise:
an `ADMIN_TOKEN` request reaches tezgah as `Actor::Staff` carrying the nil
uuid, so an audit row written under it says plainly that nobody in
particular did it.

The admin surface is mounted when there is any way in — a token, or at least
one operator. With neither it is not bound at all: genuinely absent rather
than present and refusing everybody, so there is nothing for a stranger to
find. Startup says which of the two it found.

| Route | Open? | What it does |
|---|---|---|
| `POST /auth/session` | yes | e-mail and password in, a session token out |
| `DELETE /auth/session` | no | ends the session doing the asking |
| `GET /auth/me` | no | who the caller is; `null` for `ADMIN_TOKEN` |
| `POST /auth/password` | no | changes the caller's own, ending every other session they hold |
| `GET /admin/operators` | no | the accounts, and which are disabled |
| `GET /admin/records/audit` | no | who did what to which row, newest first — owner only |
| `GET /admin/records/events` | no | the outbox, newest first — owner only |
| `POST /admin/operators` | no | makes one — owner only |
| `POST /admin/operators/{id}/password` | no | sets somebody else's — owner only, and ends every session they hold |
| `PATCH /admin/operators/{id}` | no | changes a role, disables or re-enables one — owner only, never itself, never the last owner |

None of these is one of tezgah's 486 — the crate declares no route for
something it does not do — so the startup tally does not count them, the same
way it does not count `GET /health`.

**No invitation, and the reset is a person rather than a link.** Both would
need a letter and this binary has no mailer; a link it cannot send would be
worse than one it never offered. So an account is made with a password by
somebody already inside, and an operator who forgets theirs has an owner set
a new one — told to them the same way the first one was. Every session that
operator holds ends with it, including the one they may be sitting in: an
account whose password was reset by somebody else is an account that may have
been taken.

`ADMIN_TOKEN` is still the way back in when there is no owner left to ask.

**Three roles, checked at the door.**

| Role | May |
|---|---|
| `owner` | anything, and the only role that may make or disable an account |
| `staff` | the day-to-day — reading, writing, deleting, moderating. Not moving money |
| `viewer` | reading |

The split is tezgah's own rather than one invented here: `ports::Action`
already separates `Settle` — capture, refund, cancel — from `Write`, because
"editing an order and refunding one are not one power". The gate reads the
`Action` each route declares in `tezgah::api::routes()`, the same table the
OpenAPI document and the permission matrix read, so a role is checked against
what the route says rather than against a second list kept here and drifting
from it.

The first account made is the owner whatever was asked for, and the last
owner can be neither demoted nor disabled: a shop whose only account cannot
make a second has locked itself out with the key inside, and the way back
would be the `ADMIN_TOKEN` it was told it could stop keeping. `ADMIN_TOKEN`
itself counts as an owner, and has to.

**This is authorization at the door, not at the row.** It answers "may this
person refund anything at all"; it does not answer "may this person refund
*this* order". That second question is `tezgah::ports::Authorizer`'s, and
`ServerHost` answers it by granting everything.

Writing a real authorizer here would be dead code, and it is worth saying why
rather than leaving it as an obvious next step. `Resource` carries the owner
on the five kinds that have one — cart, order, payment, credit, subscription
— so a per-row rule needs no lookup, only an actor to compare against. This
binary has none to compare: the back office is `Actor::Staff`, and the
storefront is `Actor::Guest { cart }` with the cart id taken from the same
path parameter the rule would be asked about. Actor and resource agree by
construction.

What would make it bite is a storefront sign-in — an `Actor::Customer` whose
id came from a session rather than from the URL. That is a feature this
binary does not have, and it comes before the authorizer rather than after.
#214 is where that is argued.

## What runs without being asked

`src/schedule.rs`, every five minutes, as `Actor::System`:

- `cart::expire` — abandoned carts, and the stock their lines were holding.
- `inventory::expire_reservations` — every hold whose time has run out.
- expired sessions, dropped.

Both sweeps are things `tests/reachable.rs` in the crate root tolerates with
the reason "a sweep a host runs on a schedule". This is that host; until it
ran them, an abandoned cart on the shipped image was never cleared and the
stock it reserved was held for ever.

Not jobs. `ports::Jobs` is enqueue-only by design — tezgah writes a job in
the transaction the change belongs to and never decides when it runs — so
recurrence is the host's and lives here.

The queue is separate and runs beside it. `host::Dispatcher` matches on a
job's kind; a job that fails records its reason, waits a doubling backoff,
and after five attempts is left dead with that reason still on it. A kind
nothing handles fails the same way rather than being marked done — which is
what the worker used to do to every job it claimed, including the one kind
tezgah enqueues, so a declined subscription renewal was retried never.

Dispatching that one needs a `RecurringProvider`, and there is none. Charging
a card a shopper left on file means naming which card, and kasapay 0.0.5 —
the version this crate pins, and the newest published — has no field for one.

It is not missing from kasapay, only from every version of it: the commit
adding `ChargeRequest::instrument` landed eleven hours after v0.0.5 was
tagged, so it is on main and in no release. productdevbook/kasapay#225 asks
for one.

Naming the customer alone and calling it a stored charge is the "accept a
field and drop it" kasapay's own documentation refuses, so `src/provider.rs`
implements neither and says so where somebody would look for it. Until there
is a release a dunning retry records exactly that as its reason — still an
improvement on being marked done by a worker that did nothing.

## A provider calls back

`POST /webhooks/payments/{provider}`, mounted only when
`TEZGAH_PAYMENT_WEBHOOK_SECRET` is set. Any payment confirmed asynchronously —
3-D Secure, a hosted form, a bank transfer — is confirmed here.

It is neither the storefront's surface nor the back office's, and the route
table says so: `Surface::Webhook`. A shopper's publishable key and an
operator's token both mean nothing to it. What it checks is
`x-provider-signature` against the body's exact bytes, in constant time, and
refuses with the same answer whether the header was missing, malformed or
wrong — an endpoint that replies differently to a near-miss tells whoever is
guessing that they are close.

    POST /webhooks/payments/demo-bank
    x-provider-signature: sha256=<hmac of the exact body>

    {"event_id": "evt_…", "event_type": "payment_intent.succeeded",
     "kind": "authorized", "session_id": null, "amount": null,
     "payload": { …the provider's own body… }}

`kind` is one of `authorized`, `captured`, `refunded`, `canceled`, `failed` or
`other`. `payload` is kept verbatim: the audit trail wants what arrived rather
than what tezgah understood of it.

**A redelivery lands once.** The write is `on conflict do nothing` against the
unique `(scope, provider, event_id)`, so a second arrival writes no row and
answers `{"recorded": false}` — acknowledged, and nothing changed. That is the
whole reason a callback goes through a table rather than straight into a
capture.

**Recorded, not acted on.** Capturing, moving an order's state, anything that
follows from what the provider *said* is a second step against a row that is
now durable, so a crash between the two resumes rather than loses.
`GET /admin/payment-webhooks` hands back what has arrived and not been acted
on, and `POST /admin/payment-webhooks/{id}/processed` says one is done. What
does the acting is still a shop's to write, and `docs/architecture.md` counts
that as the open half.

Unset secret, unmounted route — a 404 rather than an endpoint that believes
anybody. A provider retries a 404 and says so on its dashboard; an unsigned
endpoint accepts a forged capture quietly.

## Events leave the building

`ports::EventSink` writes a row in `server_event`, inside the transaction of
the change that caused it. That is what makes an event a thing that happened
rather than a thing somebody hoped happened — a rollback takes the row with
it. Delivering it is this binary's, and `src/deliver.rs` is that.

Set `TEZGAH_EVENT_WEBHOOK` and `TEZGAH_EVENT_SECRET` and a worker posts every
undelivered row:

    POST <your url>
    content-type: application/json
    tezgah-signature: sha256=<hmac of the exact body>

    {"id": "…", "name": "order.paid", "entity_id": "…", "payload": {…}}

Verify the signature over the **raw bytes**, not over what you parsed — a body
re-serialised by your framework is the classic way a valid signature stops
matching. The secret is required: startup fails if a webhook is set without
one, because an unsigned webhook is an endpoint anybody who guesses the URL
can post to.

**At least once, never exactly once.** The row is marked delivered after you
answer, so a crash in between sends it again. `id` is in the body and does not
change between attempts — deduplicate on it, the same way tezgah's own
`payment::record_webhook` deduplicates a provider's redelivery on the way in.

Anything other than a 2xx is a failure: the row keeps the reason, waits a
doubling backoff from a minute, and after five attempts is left dead with that
reason still on it. `/admin/records/events` shows all of it.

One destination on purpose. A shop that needs events in five places puts
something that fans out behind the one URL. Left unset, every event is still
written down and readable — which is what this binary did before there was a
deliverer, and is the honest default: an event posted nowhere in particular is
worse than one left in a table somebody can read.

There is no mailer here, so an invitation, a notification and a reset link
still cannot be sent. A shop can hang mail off this webhook; that it has to is
a gap, and `docs/architecture.md` counts it as one.

## Route table

`tezgah::api::routes()` names 486 operations. This binary binds a fraction,
by hand, and says exactly how many out loud at startup:

```
bound 116 of 486 declared routes
  GET    /store/products
  GET    /store/products/{handle}
  POST   /store/carts
  GET    /store/carts/{id}
  POST   /store/carts/{id}/line-items
  GET    /store/carts/{id}/line-items
  POST   /store/carts/{id}/complete
  GET    /admin/products
  GET    /admin/products/{id}
  PATCH  /admin/products/{id}
  DELETE /admin/products/{id}
  GET    /admin/orders
  GET    /admin/orders/{id}
  GET    /admin/inventory-items
  GET    /admin/inventory-items/{id}
  DELETE /admin/inventory-items/{id}
  GET    /admin/customers
  GET    /admin/customers/{id}
  PATCH  /admin/customers/{id}
  DELETE /admin/customers/{id}
  GET    /admin/promotions
  GET    /admin/promotions/{id}
  PATCH  /admin/promotions/{id}
  DELETE /admin/promotions/{id}
  GET    /admin/subscriptions
  GET    /admin/subscriptions/{id}
  GET    /admin/regions
  GET    /admin/regions/{id}
  PATCH  /admin/regions/{id}
  GET    /admin/sales-channels
  GET    /admin/sales-channels/{id}
  PATCH  /admin/sales-channels/{id}
  DELETE /admin/sales-channels/{id}
  GET    /admin/currencies
  GET    /admin/publishable-api-keys
  GET    /admin/stock-locations
  PATCH  /admin/stock-locations/{id}
  DELETE /admin/stock-locations/{id}
  POST   /admin/currencies
  POST   /admin/regions
  POST   /admin/sales-channels
  POST   /admin/publishable-api-keys
  POST   /admin/stock-locations
  POST   /admin/products
  POST   /admin/products/{id}/variants
  POST   /admin/price-sets
  POST   /admin/product-variants/{id}/price-set
  POST   /admin/prices
  POST   /admin/inventory-items
  POST   /admin/inventory-items/{id}/location-levels
  GET    /admin/order-baskets/{id}
  GET    /admin/order-baskets/{id}/orders
  GET    /admin/order-baskets/{id}/carts
  GET    /admin/workflows-executions
  GET    /admin/workflows-executions/{id}
  GET    /admin/workflows-executions/{id}/steps
  GET    /admin/workflow-dead-letters
  GET    /admin/commission-rules
  GET    /admin/orders/{id}/payout-lines
  GET    /admin/payouts
  GET    /admin/payout-balance/{currency_code}
  GET    /admin/orders/{id}/fulfillments
  GET    /admin/orders/{id}/shipping-options
  GET    /admin/orders/{id}/returns/shipping-options
  GET    /admin/orders/{id}/fulfillments/{fulfillment_id}
  GET    /admin/fulfillment-sets
  GET    /admin/fulfillment-sets/{id}/service-zones
  GET    /admin/fulfillment-providers
  GET    /admin/shipping-options
  GET    /admin/shipping-options/{id}
  GET    /admin/shipping-options/{id}/translations
  GET    /admin/shipping-options/{id}/translations/{locale}
  GET    /admin/shipping-profiles
  GET    /admin/shipping-profiles/{id}
  GET    /admin/shipping-option-types
  GET    /store/shipping-options
  GET    /admin/tax-regions
  GET    /admin/tax-regions/{id}
  GET    /admin/tax-rates
  GET    /admin/tax-rates/{id}
  GET    /admin/tax-rates/{id}/rules
  GET    /admin/tax-registrations
  GET    /admin/customers/{id}/tax-ids
  GET    /admin/customers/{id}/tax-exemptions
  GET    /admin/price-sets/{id}
  GET    /admin/price-sets/{id}/prices
  GET    /admin/product-variants/{id}/bundle/components
  GET    /admin/product-variants/{id}/bundle/price
  GET    /admin/prices/{id}/rules
  GET    /admin/price-lists
  GET    /admin/price-lists/{id}
  GET    /admin/price-preferences
  GET    /admin/payments
  GET    /admin/payments/{id}
  GET    /admin/payments/payment-providers
  GET    /admin/payment-collections/{id}
  GET    /admin/payment-collections/{id}/payment-sessions
  GET    /admin/refund-reasons
  GET    /store/payment-providers
  GET    /admin/gift-cards
  GET    /admin/gift-cards/{id}
  GET    /admin/gift-cards/{id}/transactions
  GET    /admin/customers/{id}/store-credit
  GET    /admin/store-credits/{id}/transactions
  GET    /store/carts/{id}/credits
  GET    /admin/orders/{id}/entitlements
  POST   /admin/orders/{id}/entitlements/revoke
  GET    /admin/variants/{id}/digital-content
  POST   /admin/variants/{id}/digital-content
  DELETE /admin/digital-content/{id}
  GET    /admin/carts
  plus GET /health, which is this binary's own and not one of the 486
```

That is the count with `ADMIN_TOKEN`, `TEZGAH_STOCK_LOCATION_ID` and
`TEZGAH_DEMO_BANK` all set. Without any one of them, the corresponding rows
above are not bound and the startup count drops accordingly — the log line
is always the true count for that run, never a number copied from here.

**Store — the shopping flow.** The same walk `../../examples/shop` makes as plain
calls: browse the catalogue with a publishable key
(`x-publishable-key` header), open a cart, add a line, check out.

**Admin — one list per screen, the single read behind each row, plus what
fills a shop.** [`client/`](../client) is the admin panel this repository
ships, and it draws seven screens: products, orders, inventory, customers,
promotions, subscriptions, and store (which reads two lists of its own,
regions and sales channels, plus the currencies list its overview reads).
Eleven of the thirty-one admin routes are list endpoints — the original nine,
plus `GET /admin/publishable-api-keys` and `GET /admin/stock-locations`,
which the panel needed and this binary did not yet bind: without the first,
an operator who loses a publishable key can only mint another, never see
whether one already exists. Eight more are the single-row read behind a
click on one of those lists' rows — `GET /admin/{products,orders,
inventory-items,customers,promotions,regions,sales-channels,subscriptions}/
{id}` — so a screen's detail view has somewhere to fetch from instead of
nowhere. The other twelve are #214's list: enabling a currency, opening a
region, a sales channel and a stock location, minting a publishable key, and
creating a product, its variants, a price set, a price and a stocked
inventory level — the smallest set that gets a fresh install to something a
storefront can check out from. `tezgah-server seed` (above) does the first
five of those in one command; the rest — a real catalogue — go in through
these routes, by hand or by whatever the panel or a script does with them.

**Editing and deleting a row, wherever `tezgah::api` has the function for
it.** None of the seven screens could change or remove what they list before
this: `PATCH /admin/products/{id}` (`admin_catalogue::update_product`),
`/admin/customers/{id}` (`admin_rest::update_customer`),
`/admin/promotions/{id}` (`update_promotion`), `/admin/regions/{id}`
(`update_region`) and `/admin/sales-channels/{id}` (`update_sales_channel`)
all ask `Action::Write`; `/admin/stock-locations/{id}`
(`admin_catalogue::rename_stock_location`, also `Action::Write`) is narrower
— a stock location's only editable field past its address is its name.
`DELETE` follows the same five domains but regions
(`admin_catalogue::delete_product`, `delete_stock_location`,
`admin_rest::delete_sales_channel`, `delete_promotion`, `delete_customer`),
plus inventory items, which has a delete and no update
(`admin_catalogue::delete_inventory_item`) — all `Action::Delete`, and all
soft: `delete_product`, `delete_inventory_item` and `delete_customer` set
`deleted_at` and leave the row and what points at it in place;
`delete_promotion` is a withdrawal, so the discounts it already granted stay
on the orders that used them; `delete_sales_channel` refuses a shop's default
channel; `delete_stock_location` refuses a location that still counts stock.

Two of the seven have neither. Currencies have no writer past
`create_currency` — nothing in `src/api/` updates or removes one once
enabled. Publishable API keys have `POST /admin/publishable-api-keys/{id}/
revoke`, which withdraws a key without forgetting it, but that route asks
`Action::Write` on a `POST`, not `Action::Delete` on a `DELETE`, so it is not
what this table is counting — and there is no update for a key's title
either. Regions get the update above but no delete: `tezgah::api` has a route
to take a country out of a region, never one to remove the region itself.
Inventory items get the delete above but no update: the only write past
creation is to the stock a location holds of an item, already bound at
`POST /admin/inventory-items/{id}/location-levels`.

**Past the panel: reads with no screen yet.** Ten domains had list-and-
single-read functions sitting in `src/api/` with nothing in this binary
calling them — reachable from a test, not from a request. Each is bound as
its functions allow, never inventing a list or a single read a domain does
not already offer:

- **order_basket** — a basket's own record and the two scope-local lists
  under it (`order_basket::get_basket`, `basket_orders`, `basket_carts`).
  There is no `list` across baskets in the crate; only these three.
- **workflow** — a run's list, single read and steps
  (`admin_rest::list_workflow_runs`, `get_workflow_run`,
  `list_workflow_run_steps`), plus the scope-wide dead-letter list
  (`list_workflow_dead_letters`).
- **payout** — a seller scope's own commission rules, one order's payout
  lines, its payout history and its balance in one currency
  (`payout::commission_rules`, `order_payout_lines`, `payouts`, `balance`).
- **fulfilment** — a parcel's list and single read on one order, the
  shipping options that reach an order's address and a return's,
  fulfilment sets and their service zones, which carriers are on, shipping
  options and their translations, shipping profiles, and shipping option
  types (`admin_order::order_fulfillments`, `get_fulfillment`,
  `order_shipping_options`, `return_shipping_options`,
  `list_fulfillment_sets`, `service_zones`, `fulfillment_providers`,
  `list_shipping_options`, `get_shipping_option`,
  `list_shipping_option_translations`, `localised_shipping_option`,
  `list_shipping_profiles`, `get_shipping_profile`,
  `list_shipping_option_types`), plus the storefront's own
  `GET /store/shipping-options` (`store::list_shipping_options`), which
  prices delivery for a cart rather than reading a back office's
  configuration.
- **tax** — tax regions and rates, list and single read on both, and the
  rules on one rate (`admin_rest::list_tax_regions`, `get_tax_region`,
  `list_tax_rates`, `get_tax_rate`, `list_tax_rate_rules`), plus where the
  shop is registered and what it files under, a customer's tax numbers and
  their exemption certificates, from `src/api/tax_identity.rs`
  (`list_registrations`, `list_tax_ids`, `list_exemptions`). None of the
  three in `tax_identity` has a single-row read by id in the crate — a
  registration, a tax number and a certificate are read as their owner's
  whole list, never one at a time.
- **pricing** — one price set and the page of prices under it, a bundle's
  components and what it prices at right now, the rules on one price, price
  lists with a list and single read on both, and a price preference found
  by attribute rather than by id (`admin_catalogue::get_price_set`,
  `list_prices`, `list_bundle_components`, `bundle_price`,
  `list_price_rules`, `list_price_lists`, `get_price_list`,
  `get_price_preference`). The last answers `null` rather than a 404: no
  preference set for an attribute is the common case, not a missing row.
- **payment** — payments with a list and single read, which carriers a
  shop accepts, a payment collection's single read and the sessions under
  it, and refund reasons (`admin_order::list_payments`, `get_payment`,
  `payment_providers`, `get_payment_collection`, `payment_sessions`,
  `list_refund_reasons`), plus the storefront's own
  `GET /store/payment-providers` (`store::list_payment_providers`), narrowed
  to a cart's region the same way `GET /store/shipping-options` narrows to
  its address. `CollectionView`'s four running totals — the collection's
  amount, and what has been authorized, captured and refunded against it —
  are the payment domain's own raw-`Decimal` money fields, so
  `money_crosses_the_wire_as_a_string_not_a_number` grows four more
  pointers for them.
- **credit** — gift cards with a list, single read and their transactions,
  and a customer's store-credit balance in one currency with its own
  transactions (`credit::list_gift_cards`, `get_gift_card`,
  `gift_card_movements`, `get_store_credit`, `store_credit_movements`),
  plus what a cart currently means to pay with,
  `GET /store/carts/{id}/credits` (`list_cart_credits`).
  `GET /store/customers/me/store-credit` (`my_store_credit`) is not among
  these: it calls `signed_in(ctx)`, which only ever succeeds for
  `Actor::Customer`, and this binary has no customer sign-in anywhere —
  every storefront route it binds runs as `Actor::Guest`. Binding it would
  mean a route that answers `denied` to every caller, which is worse than
  leaving it unbound; giving this binary a customer identity is a separate
  decision, the same shape as #214.
- **digital** — what an order's money bought the right to, a list and a
  hand revocation, and a variant's own files, list, write and delete
  (`digital::list_order_entitlements`, `revoke_entitlements`,
  `list_content`, `put_content`, `delete_content`). Writes are bound here,
  past this round's read-only rule for the other nine domains, because the
  domain had no route at all — a shop with digital products could not
  reach any of it. The storefront half —
  `GET /store/entitlements`, `POST /store/entitlements/{id}/token`,
  `POST /store/downloads` (`my_entitlements`, `create_token`, `redeem`) —
  is not bound for the same reason `GET /store/customers/me/store-credit`
  above is not: all three call `signed_in(ctx)` before anything else, and
  answer `denied` to the `Actor::Guest` every storefront route in this
  binary runs as. Binding a route that refuses its every caller would be
  worse than the gap it replaces.
- **cart** — a cart's own line items, `GET /store/carts/{id}/line-items`
  (`store::list_line_items`), and every cart this scope holds, abandoned
  ones included, `GET /admin/carts` (`order_basket::list_carts`, so named
  because that file already imports `crate::cart` and reaches for
  `CartView` on every other route in it). The second did not exist as a
  route at all until now: `cart::list` sat in `tests/reachable.rs`'s
  `TOLERATED` list with the reason "the back office has no cart screen" —
  no longer true, so the entry left with it, and the list is 33 long
  rather than 34.

Everything else `tezgah::api` offers stays unbound; wiring in more of the
486 is a matter of adding a handler in `src/http/admin.rs` or
`src/http/store.rs`, not a limitation of the approach.

Every one of the eight single-row reads the panel's own screens use asks
`ctx.permit(..)` before it looks
for the row, not after: `tests/api_permissions.rs` calls each of
`admin_catalogue::get_product`, `admin_order::get_order`,
`admin_catalogue::get_inventory_item`, `admin_rest::get_customer`,
`admin_rest::get_promotion`, `admin_rest::get_region`,
`admin_rest::get_sales_channel` and `subscription::get_subscription` with a
random id and a host that refuses everything, and asserts `denied` rather
than `not_found` — none of the eight is in that test's `TOLERATED` list, so
none of them is the leak `CLAUDE.md` describes, where a missing row answers
`not_found` without asking and an existing-but-not-yours row asks and answers
`denied`, together telling a caller which ids exist.

## Docker

```sh
docker build -f app/server/Dockerfile -t tezgah-server .
docker run --rm -p 8080:8080 \
    -e DATABASE_URL=postgres://postgres:postgres@host.docker.internal:5432/tezgah \
    -e ADMIN_TOKEN=... \
    tezgah-server
```

Multi-stage: a `rust:1-slim-bookworm` builder compiles the dependency graph
in its own cached layer before a line of tezgah's own source is copied in,
and the runtime image is `gcr.io/distroless/cc-debian12:nonroot` — no shell,
no package manager, CA roots for sqlx's TLS connection to Postgres, and the
`nonroot` tag's non-root user rather than a root default. No `HEALTHCHECK`:
that is what a k8s probe against `GET /health` is for.

Not built on the machine this was written on, and not built by this change
either — `.github/workflows/ci.yml` is what proves it compiles.
