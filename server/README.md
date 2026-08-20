# tezgah-server

A binary. `tezgah` (the crate one directory up) is a Rust library — it
decides nothing about how it is run, and owns no `main`. Somebody self-hosting
it needs a container that starts, reads its configuration from the
environment, opens a connection pool, runs migrations, and answers HTTP. This
is that container, in the same way `mavi-operator` is the hosting half over
`mavi`: a library plus the smallest honest thing that turns it into a service.

It is not the only way to run tezgah, and it does not try to be the complete
one. `../examples/shop` is the other end of the same idea, kept deliberately
small: no router, no listener, five `tezgah::api::store` functions called
directly from a `main` that seeds one shop and walks catalogue → cart →
checkout → order as plain library calls. Read that file to see what embedding
tezgah looks like at its smallest. Read this one to see it made into
something a container starts.

## Running it

```sh
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/tezgah
export ADMIN_TOKEN=$(openssl rand -hex 32)
export TEZGAH_STOCK_LOCATION_ID=<a stock location's uuid>
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
| `ADMIN_TOKEN` | no | unset | the bearer token that unlocks `/admin/*` — see below |
| `TEZGAH_STOCK_LOCATION_ID` | no | unset | the one warehouse checkout reserves and ships from — see below |
| `TEZGAH_CURRENCY_EXPONENT` | no | `2` | this shop's one currency's decimal places, for the payment provider wrapper |

Configuration comes from the environment and nowhere else: no config file
format, because a container is not handed one separately from the
environment it was started with.

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

Checkout also needs a payment provider, and ships with a stand-in:
`src/provider.rs`'s `DemoBank` authorises every charge immediately and
remembers nothing. `CLAUDE.md` is explicit that a provider is
[kasapay](https://github.com/productdevbook/kasapay)'s to write, not
tezgah's, and no adapter crate for a real bank or gateway lives in this
public repository — taking real money means depending on one and passing it
to `KasapayProvider::new` in `src/main.rs` in place of `DemoBank`.

## `GET /health`

Not "the process is running" — a probe can already tell that from the socket
accepting a connection — but "a query against Postgres still answers". Bound
unconditionally, unauthenticated, and not one of tezgah's own 483 declared
routes: it belongs to this binary, not to the crate.

## The admin surface, and why it can be switched off entirely

tezgah authenticates nobody — `tezgah::ports::Authorizer` is a question a
host answers, and this binary's own `ServerHost` answers it by granting every
actor, `Actor::System` included, because `docs/hosting.md` says denying that
silently stops every subscription renewal. A production server cannot leave
the back office at that, and it also should not invent a second role system
on tezgah's behalf — that is exactly the "second set of roles" tezgah's own
docs say a host should not be handed.

`ADMIN_TOKEN` is the middle: one shared secret, checked in constant time
against the `authorization: Bearer <token>` header, in front of every
`/admin/*` route. And when it is not set, the admin surface is not mounted on
the router at all — not bound and refusing every caller, genuinely absent, so
there is nothing there for a stranger to find. A closed default is the only
one that does not depend on an operator remembering to set something.

## Route table

`tezgah::api::routes()` names 483 operations. This binary binds a fraction,
by hand, and says exactly how many out loud at startup:

```
bound 15 of 483 declared routes
  GET    /store/products
  GET    /store/products/{handle}
  POST   /store/carts
  GET    /store/carts/{id}
  POST   /store/carts/{id}/line-items
  POST   /store/carts/{id}/complete
  GET    /admin/products
  GET    /admin/orders
  GET    /admin/inventory-items
  GET    /admin/customers
  GET    /admin/promotions
  GET    /admin/subscriptions
  GET    /admin/regions
  GET    /admin/sales-channels
  GET    /admin/currencies
  plus GET /health, which is this binary's own and not one of the 483
```

That is the count with both `ADMIN_TOKEN` and `TEZGAH_STOCK_LOCATION_ID` set.
Without either, the corresponding rows above are not bound and the startup
count drops accordingly — the log line is always the true count for that
run, never a number copied from here.

**Store — the shopping flow.** The same walk `examples/shop` makes as plain
calls: browse the catalogue with a publishable key
(`x-publishable-key` header), open a cart, add a line, check out.

**Admin — one list per screen.** [`client/`](../client) is the admin panel
this repository ships, and it draws seven screens: products, orders,
inventory, customers, promotions, subscriptions, and store (which reads two
lists of its own, regions and sales channels, plus the currencies list its
overview reads). This binary binds exactly the list endpoint each screen
calls and nothing past that — no fetch-by-id, no write, nothing from any
other domain `tezgah::api` offers. A shop that needs more than a read-only
back office is not what this binary is for yet; wiring in more of the 483 is
a matter of adding a handler in `src/http/admin.rs`, not a limitation of the
approach.

## Docker

```sh
docker build -f server/Dockerfile -t tezgah-server .
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
