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
| `TEZGAH_PAYMENT_PROVIDER` | no | unset | `iyzico` or `stripe`, with that provider's credentials beside it — see below |
| `TEZGAH_DEMO_BANK` | no | unset | set to exactly `i-understand-this-takes-no-money` to run checkout against the demo payment provider instead — see below |
| `TEZGAH_CURRENCY_EXPONENT` | no | `2` | this shop's one currency's decimal places, for the payment provider wrapper |
| `TEZGAH_EVENT_WEBHOOK` | no | unset | where an outbox row is posted; unset leaves every event written and unsent — see "Events leave the building" |
| `TEZGAH_EVENT_SECRET` | with the above | unset | signs the body. Startup fails if a webhook is set without one |
| `TEZGAH_PAYMENT_WEBHOOK_SECRET` | no | unset | the secret a payment provider's callback is signed with; unset leaves that route unmounted — see "A provider calls back" |
| `TEZGAH_SMTP_URL` | no | unset | lettre's own URL, `smtps://user:pass@host:465`; unset means this shop sends no letters |
| `TEZGAH_MAIL_FROM` | with the above | unset | who a letter is from |
| `TEZGAH_PANEL_URL` | with the above | unset | where the panel is, so an invitation can carry a link |
| `TEZGAH_FILE_DIR` | no | unset | a directory to store uploads in; unset means this shop stores no files — see "Where a file lives" |
| `TEZGAH_FILE_BASE_URL` | no | `/files` | what a stored file's URL starts with — a CDN in front of that directory, say |

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

Checkout also needs a payment provider. `TEZGAH_PAYMENT_PROVIDER` names one —
`iyzico` or `stripe` — and `src/bank.rs` builds the matching
[kasapay](https://github.com/productdevbook/kasapay) adapter from the
credentials beside it. tezgah writes no payment provider itself: `CLAUDE.md`
is explicit that a provider is kasapay's, and that file is the whole of what
this binary does about it — a `match` from a name onto an adapter crate,
wrapped in `KasapayProvider` so tezgah sees only its own `PaymentProvider`
trait. A name it was not built against is a startup error listing the ones it
was, never a fallback.

With no provider named, `TEZGAH_STOCK_LOCATION_ID` alone does not turn
checkout on. `src/provider.rs`'s `DemoBank` — which authorises every charge
and remembers nothing — is the deliberate way to run a checkout that takes no
money, and it needs `TEZGAH_DEMO_BANK` set to exactly
`i-understand-this-takes-no-money`; anything else, including unset or empty,
leaves `POST /store/carts/{id}/complete` unbound, and startup says which of
the two is missing. Setting both a provider and the demo is refused. See
[`docs/self-hosting.md`](../docs/self-hosting.md#taking-real-money).

## `GET /docs` and `GET /openapi.json`

The API's own description, and [Scalar](https://scalar.com) reading it.
`/openapi.json` is what `tezgah::api::openapi::document()` generates from the
route table — the same document `tests/snapshots/openapi.json` pins, so what a
running server describes and what CI reviews cannot drift apart.

Both are open. The document says which paths exist and what permission each
asks, and every one of those paths already refuses an unauthorised caller on
its own; a description that needed protecting would mean the protection *was*
the description.

Neither counts against the 487: they describe them.

**It is thinner than it looks.** The document declares every operation and, for
most of them, no request or response body at all — `productdevbook/tezgah#202`
is that gap, and Scalar renders it honestly, which is part of why it is worth
serving.

## `GET /health`

Not "the process is running" — a probe can already tell that from the socket
accepting a connection — but "a query against Postgres still answers". Bound
unconditionally, unauthenticated, and not one of tezgah's own 487 declared
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
| `POST /auth/invitation` | **yes** | turns an invitation's token into an account |
| `GET /admin/invitations` | no | who has been invited and not arrived — owner only |
| `POST /admin/invitations` | no | invites somebody by e-mail — owner only, needs a mailer |
| `GET /admin/records/audit` | no | who did what to which row, newest first — owner only |
| `GET /admin/records/events` | no | the outbox, newest first — owner only |
| `POST /admin/operators` | no | makes one — owner only |
| `POST /admin/operators/{id}/password` | no | sets somebody else's — owner only, and ends every session they hold |
| `PATCH /admin/operators/{id}` | no | changes a role, disables or re-enables one — owner only, never itself, never the last owner |

None of these is one of tezgah's 487 — the crate declares no route for
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

## Where a file lives

Unset `TEZGAH_FILE_DIR` and this binary stores none: a product's image is a
URL somebody else serves, which is what tezgah's own catalogue models and all
this ever was.

Set it and two routes appear. `POST /admin/files` takes one multipart field
and answers `{"url": "…"}`; that URL goes in the product's `thumbnail_url`,
so nothing in the catalogue changes shape. `GET /files/{name}` serves it back,
unauthenticated — an image on a storefront is public, and a signed URL for a
product photo is ceremony.

**What is not trusted.** The name the browser sent never reaches the disk: a
file is written as `<uuid>.<ext>`, and the extension comes from the content
type this binary recognises rather than from anything in the request. That is
what makes traversal impossible rather than handled, and reading one back
checks the same shape — thirty-two hex characters, a dot, one of five
extensions — before the name touches a path.

**Five types, listed rather than matched.** jpeg, png, webp, gif, avif.
`image/svg+xml` is an image by any `image/` prefix check and a script by every
other measure, and serving one back from the shop's own origin is a
cross-site scripting hole with a picture frame around it. Files are served
with the type this binary chose and `X-Content-Type-Options: nosniff`, so a
browser does not get to decide it knows better from the bytes.

8 MB a file. A directory on one disk is a deliberate ceiling: a shop
outgrowing it wants object storage, and because what is stored in the product
is an ordinary URL, moving to one is putting the bucket behind the same path.

## Inviting somebody

An owner makes an account and tells the person their password. That works, it
is what this binary did before there was a mailer, and it is still the way a
shop with no SMTP adds a colleague.

With `TEZGAH_SMTP_URL`, `TEZGAH_MAIL_FROM` and `TEZGAH_PANEL_URL` set,
`POST /admin/invitations` sends a letter instead:

    {"email": "…", "name": "…", "role": "staff"}

The link is `<panel>/?invitation=<token>` and the token is in it and nowhere
else — not in the response, not in the row (which keeps only a digest), not in
the log. An owner who loses it invites again, which replaces the open
invitation rather than adding a second: two live tokens for one person is two
ways in.

Good for seven days. `POST /auth/invitation` takes the token and a password
and makes the account, marking the invitation used in the same transaction —
so two requests arriving together cannot make two accounts from one token.
A token that never existed, one already used and one expired all get the same
refusal; which of the three it was is not the holder's business.

Without a mailer the invite route refuses outright rather than handing the
owner a token to pass along. A token that travels by whatever somebody pastes
it into is a password sent in the clear.

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

<!-- bound-routes: 266 -->

That number is checked rather than remembered: `tests/bound_count.rs` builds
the fullest router this binary can make without a payment provider, counts
what it mounted out of `tezgah::api::routes()`, and fails if the comment above
disagrees. It moved by hand six times in one day before that existed, and was
wrong on at least two of them.

Configuring checkout adds one more — `POST /store/carts/{id}/complete` — which
is why the number is stated without it.

`tezgah::api::routes()` names 487 operations. This binary binds them by hand,
says how many out loud at startup, and here is every one:

<!-- routes:begin -->

```
GET    /store/products
GET    /store/products/{handle}
POST   /store/carts
GET    /store/carts/{id}
POST   /store/carts/{id}/line-items
GET    /store/shipping-options
GET    /store/payment-providers
GET    /store/carts/{id}/credits
GET    /store/carts/{id}/line-items
GET    /store/customers/me
GET    /store/customers/me/addresses
GET    /store/orders
POST   /webhooks/payments/{provider}
GET    /admin/products
GET    /admin/products/{id}
PATCH  /admin/products/{id}
DELETE /admin/products/{id}
GET    /admin/orders
GET    /admin/orders/{id}
GET    /admin/inventory-items
GET    /admin/inventory-items/{id}
DELETE /admin/inventory-items/{id}
GET    /admin/products/export
POST   /admin/products/batch
POST   /admin/prices/batch
POST   /admin/inventory-items/batch
GET    /admin/customers
GET    /admin/customers/{id}
PATCH  /admin/customers/{id}
DELETE /admin/customers/{id}
GET    /admin/promotions
GET    /admin/promotions/{id}
PATCH  /admin/promotions/{id}
DELETE /admin/promotions/{id}
GET    /admin/subscriptions
GET    /admin/orders/{id}/invoices
POST   /admin/orders/{id}/invoices
GET    /admin/orders/{id}/agreements
GET    /admin/orders/{id}/withdrawal
POST   /admin/promotions/{id}/status
POST   /admin/promotions/{id}/application-method
GET    /admin/customers/{id}/addresses
POST   /admin/customers/{id}/addresses
POST   /admin/customers/{id}/erase
POST   /admin/gift-cards/{id}/adjust
POST   /admin/gift-cards/{id}/disable
POST   /admin/publishable-api-keys/{id}/revoke
GET    /admin/regions/{id}/countries
POST   /admin/regions/{id}/countries
GET    /admin/stock-locations/{id}/address
POST   /admin/stock-locations/{id}/address
POST   /admin/tax-exemptions/{id}/revoke
GET    /admin/subscriptions/{id}
GET    /admin/subscriptions/{id}/events
POST   /admin/subscriptions/{id}/cancel
POST   /admin/subscriptions/{id}/pause
POST   /admin/subscriptions/{id}/resume
POST   /admin/subscriptions/{id}/skip
POST   /admin/subscriptions/{id}/swap
POST   /admin/subscriptions/{id}/deliver
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
POST   /admin/products/{id}/publish
POST   /admin/products/{id}/archive
POST   /admin/products/{id}/submit
POST   /admin/products/{id}/approve
POST   /admin/products/{id}/reject
GET    /admin/products/{id}/tags
POST   /admin/products/{id}/tags
DELETE /admin/products/{id}/tags/{tag_id}
GET    /admin/products/{id}/categories
POST   /admin/products/{id}/categories
DELETE /admin/products/{id}/categories/{category_id}
GET    /admin/products/{id}/channels
POST   /admin/products/{id}/channels
DELETE /admin/products/{id}/channels/{sales_channel_id}
POST   /admin/campaigns/{id}/budget
POST   /admin/campaigns/{id}/promotions
GET    /admin/customer-groups/{id}/customers
POST   /admin/customer-groups/{id}/customers
GET    /admin/customers/{id}/export
POST   /admin/inventory-items/{id}/transfers
GET    /admin/inventory-items/{id}/transfers
POST   /admin/price-lists/{id}/rules
GET    /admin/products/{id}/translations
POST   /admin/products/{id}/translations
GET    /admin/product-categories/{id}/translations
POST   /admin/product-categories/{id}/translations
POST   /admin/product-variants/{id}/bundle
GET    /admin/product-variants/{id}/inventory-items
POST   /admin/product-variants/{id}/inventory-items
GET    /admin/publishable-api-keys/{id}/sales-channels
POST   /admin/publishable-api-keys/{id}/sales-channels
POST   /admin/reservations/{id}/fulfil
GET    /admin/draft-orders
POST   /admin/draft-orders
GET    /admin/draft-orders/{id}
DELETE /admin/draft-orders/{id}
POST   /admin/draft-orders/{id}/convert-to-order
GET    /admin/draft-orders/{id}/edit
POST   /admin/draft-orders/{id}/edit
DELETE /admin/draft-orders/{id}/edit
POST   /admin/draft-orders/{id}/edit/confirm
POST   /admin/draft-orders/{id}/edit/items
DELETE /admin/draft-orders/{id}/edit/items/{action_id}
POST   /admin/draft-orders/{id}/edit/shipping-methods
DELETE /admin/draft-orders/{id}/edit/shipping-methods/{action_id}
GET    /admin/exchanges
POST   /admin/exchanges
GET    /admin/exchanges/{id}
GET    /admin/exchanges/{id}/items
POST   /admin/exchanges/{id}/cancel
POST   /admin/exchanges/{id}/request
POST   /admin/exchanges/{id}/inbound/items
DELETE /admin/exchanges/{id}/inbound/items/{action_id}
POST   /admin/exchanges/{id}/inbound/shipping-method
POST   /admin/exchanges/{id}/outbound/items
DELETE /admin/exchanges/{id}/outbound/items/{action_id}
POST   /admin/exchanges/{id}/outbound/shipping-method
GET    /admin/order-edits/{id}
DELETE /admin/order-edits/{id}
POST   /admin/order-edits/{id}/confirm
POST   /admin/order-edits/{id}/items
DELETE /admin/order-edits/{id}/items/{action_id}
POST   /admin/order-edits/{id}/shipping-method
DELETE /admin/order-edits/{id}/shipping-method/{action_id}
GET    /admin/claims
POST   /admin/claims
GET    /admin/claims/{id}
GET    /admin/claims/{id}/items
GET    /admin/claims/{id}/lines
POST   /admin/claims/{id}/cancel
POST   /admin/claims/{id}/request
POST   /admin/claims/{id}/claim-items
DELETE /admin/claims/{id}/claim-items/{action_id}
POST   /admin/claims/{id}/inbound/items
DELETE /admin/claims/{id}/inbound/items/{action_id}
POST   /admin/claims/{id}/inbound/shipping-method
POST   /admin/claims/{id}/outbound/items
DELETE /admin/claims/{id}/outbound/items/{action_id}
POST   /admin/claims/{id}/outbound/shipping-method
GET    /admin/returns
POST   /admin/returns
GET    /admin/returns/{id}
GET    /admin/returns/{id}/items
POST   /admin/returns/{id}/receive
POST   /admin/returns/{id}/dismiss-items
POST   /admin/returns/{id}/cancel
POST   /admin/returns/{id}/request
POST   /admin/returns/{id}/request-items
DELETE /admin/returns/{id}/request-items/{action_id}
POST   /admin/returns/{id}/receive-items
DELETE /admin/returns/{id}/receive-items/{action_id}
POST   /admin/returns/{id}/shipping-method
DELETE /admin/returns/{id}/shipping-method/{action_id}
GET    /admin/return-reasons
POST   /admin/return-reasons
GET    /admin/return-reasons/{id}/translations
POST   /admin/return-reasons/{id}/translations
GET    /admin/return-reasons/{id}/translations/{locale}
DELETE /admin/return-reasons/{id}/translations/{locale}
POST   /admin/returns/{id}/withdrawal
POST   /admin/selling-plan-groups/{id}/plans
GET    /admin/selling-plan-groups/{id}/plans
POST   /admin/selling-plans/{id}/variants
GET    /admin/products/{id}/images
POST   /admin/products/{id}/images
GET    /admin/products/{id}/options
POST   /admin/products/{id}/options
GET    /admin/product-variants/{id}/images
POST   /admin/product-variants/{id}/images
GET    /admin/product-variants/{id}/options
POST   /admin/product-variants/{id}/options
POST   /admin/product-options/{id}/values
GET    /admin/product-categories/{id}/subtree
POST   /admin/product-categories/{id}/move
PATCH  /admin/inventory-items/{id}/tracking
GET    /admin/inventory-items/{id}/lots
POST   /admin/inventory-items/{id}/lots
GET    /admin/inventory-lots/{id}/orders
POST   /admin/inventory-lots/{id}/reservations
GET    /admin/products/{id}/variants
POST   /admin/products/{id}/variants
POST   /admin/price-sets
POST   /admin/product-variants/{id}/price-set
POST   /admin/prices
POST   /admin/inventory-items
GET    /admin/inventory-items/{id}/location-levels
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
GET    /admin/payment-webhooks
POST   /admin/payment-webhooks/{id}/apply
POST   /admin/payment-webhooks/{id}/processed
GET    /admin/payment-collections/{id}
GET    /admin/payment-collections/{id}/payment-sessions
GET    /admin/refund-reasons
GET    /admin/gift-cards
GET    /admin/gift-cards/{id}
GET    /admin/gift-cards/{id}/transactions
GET    /admin/customers/{id}/store-credit
GET    /admin/store-credits/{id}/transactions
GET    /admin/orders/{id}/entitlements
POST   /admin/orders/{id}/entitlements/revoke
GET    /admin/variants/{id}/digital-content
POST   /admin/variants/{id}/digital-content
DELETE /admin/digital-content/{id}
GET    /admin/carts
```

<!-- routes:end -->

That list is the router's rather than a transcription of it:
`tests/bound_count.rs` builds the same fullest router, renders what it
mounted the same way, and fails on any line that differs. It used to be a
paste of one run's startup log, and by the time anybody noticed it named 112
of the 253 routes bound and described a panel with seven screens.

**What decides whether a surface is there at all.** Store is always mounted.
The webhook is mounted only with `TEZGAH_WEBHOOK_SECRET` set, because a
callback endpoint with no secret is one anybody can post to. Admin is mounted
when `ADMIN_TOKEN` is set or an operator account exists — a shop with neither
has no door rather than a door that refuses everybody. `/health`,
`/openapi.json`, `/docs` and the two file routes are this binary's own, not
`tezgah::api::routes()`'s, and are logged and counted apart.

**Store — the shopping flow, and a shopper's own record.** Browse the
catalogue with a publishable key (`x-publishable-key`), open a cart, add a
line. Registering, signing in and out are bound too, and behind that session
the `Shopper` extractor turns a bearer token into `Actor::Customer` for the
three routes that read what is yours: your record, your addresses, your
orders. `POST /store/carts/{id}/complete` is bound only once checkout is
configured — that needs a payment provider and a warehouse — which is why it
is not in the list above and the count is stated without it.

**Admin — every list, the read behind each row, and the writes each domain
actually offers.** No route here was invented: a handler exists where
`tezgah::api` has the function, and a domain that offers no update has no
`PATCH`. Deletes are soft where the domain makes them soft — `delete_product`,
`delete_inventory_item` and `delete_customer` set `deleted_at` and leave what
points at the row in place; `delete_promotion` is a withdrawal, so discounts
already granted stay on the orders that used them; `delete_sales_channel`
refuses a shop's default channel and `delete_stock_location` a location that
still counts stock.

**What is deliberately not bound.** 221 of the 487 remain, and all but three
are simply not reached yet — binding is by hand, a batch at a time. The three
are held back on purpose:

- `POST /admin/store-credits/{id}/adjust` — the path names a store credit and
  `credit::adjust_store_credit` takes a customer id. One of the two is wrong,
  and a handler that picks a winner would settle it in the wrong place.
- `POST /admin/subscriptions/{id}/renew` and `POST /admin/subscriptions/{id}/card` —
  both need a `RecurringProvider`, and kasapay cannot yet name a saved card
  (productdevbook/kasapay#225). Neither is a manual way around the dunning
  gap: the same missing trait blocks both.

The storefront's signed-in long tail — `GET /store/customers/me/store-credit`,
`GET /store/entitlements`, `POST /store/entitlements/{id}/token`,
`POST /store/downloads` — is unbound for no reason beyond the batch it belongs
to. Each calls `signed_in(ctx)`, which the `Shopper` extractor above now
satisfies; before customer accounts existed they would have answered `denied`
to every caller, and this file said so for as long as that was true.

A single-row read asks `ctx.permit(..)` before it looks for the row, never
after. `tests/api_permissions.rs` calls each with a random id and a host that
refuses everything and asserts `denied` rather than `not_found` — including
all eight the panel's detail screens use (`admin_catalogue::get_product`,
`admin_order::get_order`, `admin_catalogue::get_inventory_item`,
`admin_rest::get_customer`, `get_promotion`, `get_region`,
`get_sales_channel`, `subscription::get_subscription`), none of which is in
that test's `TOLERATED` list. The list is not empty, though: the routes still
in it answer `not_found` without asking, and a pair of answers that differ
tells a stranger which ids exist.

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
