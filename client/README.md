# client

tezgah's admin panel. React 19, Vite, TanStack Router and Query, shadcn.

## What it talks to

Nothing, yet. tezgah is a library and serves no HTTP itself — something has to
mount `api::routes()` first, which is what the repository's issue #199 is
about. Point the panel at whatever does:

    VITE_TEZGAH_API=http://localhost:8080/api bun run dev

With nothing there, every screen says so rather than drawing an empty table.

## Types

`tests/snapshots/openapi.json` declares 483 operations and, today, 22 of them
carry a schema — request or response — against 34 named schemas
(productdevbook/tezgah#202 is the rest). Two things are generated from it with
[Orval](https://orval.dev), by `orval.config.ts`, both reading the same
document through the same `input.override.transformer` (`orval/transformer.cjs`,
below):

- `src/api/generated/fetch/**` — a typed fetch call per operation, one file
  per tag, wired to the custom mutator at `src/api/mutator.ts`.
- `src/api/generated/zod/**` — a zod schema per operation, the same split.

`src/api/schema.d.ts` is a third, older generation, from
[`openapi-typescript`](https://openapi-typescript.pages.dev): the set of paths
that exist, so a typo cannot become a request nobody answers — the one thing
useful about the other 461 operations Orval also sees but cannot give a body
to.

    bunx openapi-typescript ../tests/snapshots/openapi.json -o src/api/schema.d.ts
    bun run generate   # orval, then the route tree — see below

CI regenerates all three and fails on a diff.

### Where a schema comes from

`src/api/schemas.ts` is the one file every screen imports from, and it says
which half of #202 each type is on. Where the document covers an operation,
the export is the generated zod schema, re-named to what the screen already
called it (`product`, `order`, ...) — `GET .../{id}` is used as the item
shape, since the corresponding list is the same shape under `page()`, not a
second declaration of it. Where the document does not — every write form's
body — the schema is transcribed by hand from the Rust struct it names, same
as before Orval existed.

Either way they are zod, parsed at the boundary (`src/api/drift.ts`): a
hand-written *or* generated type can still drift from what the server
actually answers if the two sides disagree at runtime, and parsing turns that
into its own error kind, `drifted`, naming the field, instead of `undefined`
sitting quietly in a cell.

### Orval can't read a bare JSON Schema boolean

`tests/snapshots/openapi.json` uses 2020-12's shorthand for "any value" —
`Page.items.items: true`, every `serde_json::Value` field — and Orval 8.24
throws on a literal `true`/`false` where it expects an object. `orval/transformer.cjs`
rewrites `true` → `{}` and `false` → `{not: {}}` — the same schema, spelled so
Orval's own walk over it does not throw — and it runs only on the copy Orval
parses in memory. Nothing under `tests/snapshots/` is touched.

### The custom mutator, and where each `ApiError` kind comes from

A `mutator` turns off Orval's own response handling — every generated call
becomes `return apiMutator(url, options)` — which is deliberate: `unreachable`,
`unauthenticated`, `denied`, `not_found` and `refused` need no schema, only a
status code and whether a token was sent, and `src/api/mutator.ts` decides all
five without one. `drifted` is the exception, and it is thrown one layer up,
in `src/api/drift.ts`, once a schema is available — the hand-rolled
`get`/`post` in `src/api/client.ts` (for the paths #202 has not documented a
body for) call `drift.ts` themselves, the same way a generated call would if
one had a screen wired to it yet.

## Coverage

The sidebar reads `src/lib/nav.ts`, which carries the number of admin
operations each section's tag declares. Ten sections have screens — products,
orders, inventory, customers, promotions, subscriptions, store, payouts,
workflows and baskets — which is 346 of the 483 operations. The rest say how
many they are not drawing yet, because those operations exist and work; only
the screen is missing.

Tables are TanStack Table v9, with no feature registered. Nothing sorts or
filters on the client on purpose: every list handler takes a cursor and a
limit and nothing else, so a sortable header would be claiming something about
the pages that are not on screen.

## Routing

File-based, via `@tanstack/router-plugin` — `src/routes/` and the generated
`src/routeTree.gen.ts`, which is committed: it is generated source the app
reads at runtime, not a build artefact, the same reasoning as
`src/api/generated/**`. Regenerate both with `bun run generate`; CI fails on a
diff the same way it does for `schema.d.ts`.

A route file is deliberately thin — the path, `validateSearch`, and enough
wiring to turn a route's search params into props. The screen it renders lives
in `src/features/<domain>/`, one file per screen, and never reads router
*state* itself (`useSearch`, `useParams`, a loader's data) — that still comes
in as props from the route, which is what keeps a screen shareable rather than
carrying state a URL doesn't. A screen may render a `Link` to a fixed sibling
route (a "New product" button that points at `/products/new`, a "Cancel" that
points back at the list) or call `useNavigate()` after a mutation succeeds,
the same way `components/app-shell.tsx` already builds the sidebar's own
links — pointing at a fixed address is markup, not state, and forcing it
through the route as a pre-built prop would only move the same import without
removing it. The ten built sections each have a real route; the other seven
fall through to `src/routes/$section.tsx`, which is what `NotBuilt` used to be
reached through — a static route always wins the match over a dynamic one, so
a slug that gains a screen just needs a file added here.

### A creation form is a route, not a dialog

`/products/new` and the four under `/store/*/new` (currencies, regions, sales
channels, publishable keys) are their own addresses — `src/features/<domain>/new*.tsx`,
each thin enough to be one form, one mutation, and a save/cancel that both
land back on the list. They are file-named with a trailing underscore before
the segment that would otherwise nest them —
`routes/store_.currencies.new.tsx` is `/store/currencies/new` as a sibling of
`routes/store.tsx`, not a child rendered inside its `<Outlet />` — because a
creation page is a full page, not a tab's content, and `@tanstack/router-generator`
takes that convention from Remix's flat routes. `/store/keys/new` is the one
exception to "save returns to the list": the token it mints is shown once and
never stored anywhere it could be read back, so the page shows it in place
first and only returns to `/store/keys` once the operator says "Done" —
navigating away immediately would make the token unreachable a screen ever
gets to show.

### A row is a route too

`/products/$id`, `/orders/$id`, `/inventory/$id`, `/customers/$id`,
`/promotions/$id`, `/subscriptions/$id`, `/store/regions/$id`,
`/store/sales-channels/$id` and `/workflows/$id` are the nine `GET .../{id}`
operations bound alongside their lists — `src/features/<domain>/detail.tsx`,
one screen each, reading every field the response carries rather than the
subset its list column happened to need. Their route files take the same
trailing-underscore escape the `new` routes do (`routes/products_.$id.tsx`,
`routes/store_.regions.$id.tsx`), for the same reason: a list's own route is
not a layout, and `/store`'s tabs are chrome a single record's page does not
want. `components/data-table.tsx`'s `rowLink` is what gets a table row there:
a real `Link`, `params={{ id: row.id }}`, stretched over the row's first cell
with `absolute inset-0` so the whole row is clickable — never an `onClick`,
which a middle click or "open in new tab" would do nothing with. Going back
does not re-open page one: nothing about `rowLink` touches the list's own
`after`, so `DetailHeader`'s "Back" is `router.history.back()`, landing on
whatever page of the list was open when the row was clicked.

`/baskets/$id` takes the same trailing-underscore route (`routes/baskets_.$id.tsx`)
but is not reached from a row: `order_basket` has no list endpoint, so
`/baskets` is a search box instead, and submitting it is what navigates —
see "Baskets have no list" below.

### The store's tabs are routes, not `Tabs` state

`/store/currencies`, `/store/regions`, `/store/sales-channels` and
`/store/keys` are four real routes nested under `routes/store.tsx`, which
renders `<Outlet />` inside `StoreLayout` the same way `routes/__root.tsx`
renders `<Outlet />` inside `AppShell` — a layout, not a screen, so it is
allowed to know about routing. `components/store-tabs.tsx` picks the active
tab the same way `app-shell.tsx` picks the active section (`useMatchRoute`
against the current URL), and each `Tabs.Tab` renders as a `Link` rather than
switching client state — `nativeButton={false}` tells Base UI the element
underneath is the anchor, not a button it renders itself. `/store` on its own
names no tab, so `routes/store.index.tsx` redirects to `/store/currencies`.

`/payouts` and `/payouts/commission-rules` (`components/payouts-tabs.tsx`),
and `/workflows` and `/workflows/dead-letters` (`components/workflows-tabs.tsx`),
take the same shape — except `/store` names no tab of its own and redirects,
while a scope's own payouts and a run's own executions are real content, so
`routes/payouts.index.tsx` and `routes/workflows.index.tsx` render their tab
directly instead of redirecting to a sibling.

### Baskets have no list

`order_basket` (5 operations) has no `GET /admin/order-baskets` — the crate
does not carry one; `src/api/order_basket.rs` says a shopper never asks for a
basket by id, the checkout that opened it hands the order numbers back, so
whoever needs one already has it. `/baskets` (`src/features/baskets/search.tsx`)
is a search box rather than a table, and says so rather than showing an empty
list; submitting it navigates to `/baskets/$id`, the same detail screen a row
would have opened if this section had rows. Its two sub-lists, carts and
orders, are real `Page<T>` endpoints and keep their own cursors in the
detail route's own `validateSearch` (`cartsAfter`, `ordersAfter`), the same
as any other paginated list.

### Pagination lives in the URL

Every paginated route's `validateSearch` carries `after` (products also
`status`, workflow executions also `state`), so the page a screen shows is
always the one named in its own address — `/products?status=draft&after=<cursor>`
is a link somebody else can open to the same page. The same is true of a
lookup that is not itself a list's own filter: `/payouts`'s balance and
order-lines widgets keep the currency code, the order id and the lines
cursor in its own search params too, because a balance somebody looked up is
still a page worth linking to. `src/lib/paged.ts`'s `usePagedList` still keeps a
back-stack, but only in memory, and only for the "Back" button's benefit: the
API hands out a cursor forward and nothing that walks back, so the stack is
what makes that button work, not what decides where the list is now — the
route's search param does.

## Two of shadcn's own files are patched

`src/components/ui/spinner.tsx` and `scroll-area.tsx` do not typecheck as
generated, and every `shadcn add` or preset change writes them back the way
they were. That happened twice — once at `base-luma`, once at `base-maia` — so
the fixes are in [`patches/`](patches) and `bun run patch` re-applies them.
It runs as part of `build` and `typecheck`, and is idempotent, so a forgotten
re-apply is a failing typecheck rather than a surprise.

## Scripts

    bun run dev         # vite
    bun run build       # patch, then tsc -b, then vite build
    bun run lint
    bun run typecheck
    bun run generate    # orval, then the route tree
    bun run patch
