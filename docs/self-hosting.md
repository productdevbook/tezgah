# Running tezgah yourself

This is for running the published images on your own machine — not for
embedding tezgah as a library, which is what the main [README](../README.md)
and [`docs/hosting.md`](hosting.md) are for. Two images come out of every
release: `ghcr.io/productdevbook/tezgah-server` (the API, `/admin/*` and
`/store/*`) and `ghcr.io/productdevbook/tezgah-panel` (the admin panel, built
static and served by nginx). `.github/workflows/publish.yml` builds both for
`linux/amd64` and `linux/arm64` on every push to `main` (tagged `edge`) and
every `vX.Y.Z` tag (tagged `X.Y.Z`, `X.Y` and `latest`) — each architecture on
a runner of its own, then one manifest over the two, so a tag never points at
half a build.

## Five minutes with Compose

```sh
curl -O https://raw.githubusercontent.com/productdevbook/tezgah/main/docker-compose.yml
curl -O https://raw.githubusercontent.com/productdevbook/tezgah/main/.env.example
cp .env.example .env
# edit .env: at least POSTGRES_PASSWORD and ADMIN_TOKEN
docker compose up -d
docker compose exec tezgah-server tezgah-server seed
```

That last line is what makes the shop worth opening the panel for — a fresh
install starts with no currency, no region, no sales channel and no
publishable key, and `seed` writes the smallest set of those a storefront can
check out from. `app/server/README.md`'s "Seeding a shop" section says what it
prints and why running it twice is safe.

Then make yourself an account, because `ADMIN_TOKEN` is a shared secret and
an audit row written under it names nobody:

```sh
curl -X POST http://localhost:8081/admin/operators \
  -H "authorization: Bearer $ADMIN_TOKEN" \
  -H "content-type: application/json" \
  -d '{"email": "you@example.com", "name": "You", "password": "a long one"}'
```

Sign in with that at the panel from then on. Sessions last thirty days and
end when the account is disabled or its password changes. There is no
invitation e-mail and no password reset: this server has no mailer, and a
reset link it cannot send would be worse than one it never offered — which is
why `ADMIN_TOKEN` is worth keeping somewhere safe rather than throwing away
once accounts exist.

`postgres` has to answer its own healthcheck before `tezgah-server` starts,
and `tezgah-server` has to answer `GET /health` before `tezgah-panel` starts —
`docker-compose.yml`'s `depends_on: condition: service_healthy` is why `up -d`
followed immediately by opening the panel usually still works. The panel
answers on `PANEL_HTTP_PORT` (`8080` by default); the API on `SERVER_HTTP_PORT`
(`8081` by default) for anything that wants to call `/admin` or `/store`
directly rather than through the panel's own `/api/` proxy.

## Environment variables

| Variable | Read by | Default | What it is |
|---|---|---|---|
| `POSTGRES_USER` | postgres, and built into `DATABASE_URL` | `tezgah` | the database role Postgres creates on first start |
| `POSTGRES_PASSWORD` | postgres, and built into `DATABASE_URL` | — required | change this before the first `up` — Postgres sets it once, from the empty volume, and ignores a later change to `.env` |
| `POSTGRES_DB` | postgres, and built into `DATABASE_URL` | `tezgah` | the database name |
| `ADMIN_TOKEN` | tezgah-server | — unset | the shared secret that makes the first operator account, and the way back in when a password is lost. It is not a person: an audit row written under it names nobody. With neither a token nor an operator account, tezgah-server does not serve `/admin/*` at all |
| `TEZGAH_DEMO_BANK` | tezgah-server | — unset | set to exactly `i-understand-this-takes-no-money` to run checkout against the one payment provider this binary ships — a demo that authorises every charge and takes no real money. Unset (or any other value), and `POST /store/carts/{id}/complete` is not bound at all. See [Taking real money](#taking-real-money) below |
| `SERVER_PORT` | tezgah-server, and tezgah-panel's upstream | `8080` | the port tezgah-server listens on inside its own container |
| `SERVER_HTTP_PORT` | Compose only | `8081` | the host port tezgah-server is published on |
| `PANEL_HTTP_PORT` | Compose only | `8080` | the host port the panel is published on |
| `TEZGAH_VERSION` | Compose only | `latest` | the image tag to pull — a version (`1.2.3`), `edge` for the tip of `main`, or `latest` for the newest release. Pin a real deployment to a version; `latest` moves under you |

`DATABASE_URL` itself is not a variable you set — `docker-compose.yml` builds
it from the three `POSTGRES_*` values, so there is one password to change
rather than two that can quietly disagree.

## What starts empty

tezgah-server runs its own migrations against `DATABASE_URL` on startup —
nothing to run by hand, and nothing to wait for beyond the container's own
healthcheck going green.

## Backups

Nothing here backs up the database on a schedule; that is the one thing worth
setting up yourself before this holds anything you'd miss. A plain dump:

```sh
docker compose exec postgres sh -c 'pg_dump -U "$POSTGRES_USER" -d "$POSTGRES_DB"' \
  | gzip > tezgah-$(date +%F).sql.gz
```

`$POSTGRES_USER` and `$POSTGRES_DB` there are read from the container's own
environment — the same values `.env` set — so there is nothing to pass in by
hand. Restore it into a fresh volume with:

```sh
gunzip -c tezgah-2024-01-01.sql.gz | \
  docker compose exec -T postgres sh -c 'psql -U "$POSTGRES_USER" -d "$POSTGRES_DB"'
```

## Taking real money

The published `tezgah-server` image ships exactly one payment provider:
`DemoBank`, in `app/server/src/provider.rs`. It authorises every charge it is
asked for and remembers nothing — it exists so checkout has something to run
against, not so checkout has something to take money with. Setting
`TEZGAH_STOCK_LOCATION_ID` alone does not turn it on: `TEZGAH_DEMO_BANK` also
has to be set, to exactly `i-understand-this-takes-no-money`, or
`POST /store/carts/{id}/complete` is not bound at all and startup says which
of the two is missing. Unset, empty or any other value all mean the same
thing — closed — because a phrase that has to be typed out cannot be set by
habit the way `1` or `true` can.

That default is closed on purpose. `tezgah` is public, its published images
are what a first self-host runs, and the only thing standing between a fresh
install and "every checkout succeeds and no money moved" used to be a
comment. It no longer is — but it also means there is no environment
variable that makes this image take real money. `CLAUDE.md` explains why:
payment providers are [kasapay](https://github.com/productdevbook/kasapay)'s
to write, not tezgah's, and this repository carries no adapter for a real
bank or gateway. Taking real payments means building `tezgah-server` (or your
own binary over the `tezgah` library) against a real `kasapay_core::Provider`
— an adapter crate from the kasapay project — and passing that to
`KasapayProvider::new` in `app/server/src/main.rs` in place of `DemoBank`. The
published image cannot do this for you; it is a starting point for a binary
you build, not a drop-in payment gateway.

## What this does not do

tezgah is a commerce engine, not a platform. Running the images does not get
you:

- **Authentication.** tezgah asks its `Authorizer` port whether an actor may
  do something; it does not have a login page, a session, or a password
  hash anywhere in it. Something in front of `tezgah-server` — or built into
  whatever calls it — has to decide who an `Actor` is before tezgah is asked
  what they may do.
- **E-mail.** Order confirmations, password resets, dunning notices — tezgah
  writes the event; sending anything because of it is a subscriber you add.
- **File storage.** Product images and other uploads are URLs tezgah stores,
  not bytes it keeps. Somewhere to put the files, and something to serve them
  from, is yours to run.

Each is a real host's job, addressed at
[`docs/hosting.md`](hosting.md#the-ports) and the README's own [What it asks
of you](../README.md#what-it-asks-of-you) — this page is only about running
the two images, not about what a production deployment still needs around
them.
