# client

tezgah's admin panel. React 19, Vite, TanStack Router and Query, shadcn.

Half of `app/` — see [`../README.md`](../README.md) for what the other half is
and why the two ship together. The library one directory further up knows
nothing about this one, and this one may not decide anything the library
should have decided: a total added up here, a state transition checked here, a
list sorted here because the API will not, are each a second answer to a
question Postgres already answers.

## What this panel is not, yet

The screens are real and the plumbing is honest, and it is a long way from
what an established platform's dashboard offers. That is measurable rather
than a feeling: 16,451 lines of TSX across 67 screens against 128,585 lines
and 34 locales, and the shortfall lines up with a handful of specific
absences. Some of them have moved since — what follows is where each stands.

**Filtering and searching where the API offers them; sorting on one list.**
Products, orders and customers have a search box in their address; and all three choose an
ordering: products between newest-first and by-title, orders and customers
between newest-first and by-address. Every other list orders by when a row
was written and offers no control that would say otherwise — a sortable
header on a list the API cannot sort is a claim about the pages that are not
on screen.

**Translation is a dictionary with almost nothing in it.** `panel/i18n`
holds English and Turkish and the compiler enforces that they match, and what
they cover is the shared chrome — actions, errors, the unsaved-changes
prompt. Every screen's own words are still English in the source.

**Bulk is a round trip, not a grid.** `/batch` exports a page of variants as
CSV and takes the same columns back — which is how a shop changes four
hundred prices, and it works because the export's columns and the import's
are the same. Multi-select is on the products list beside it —
`POST /admin/products/batch` takes rows to write and ids to delete, and a
bulk delete is that call with no rows. There are two edit grids, and both
exist for the same reason: their batch route takes the rows together, so the
grid is one call rather than a hundred and a half-applied page if one failed.

`/pricing/prices` types an amount in place for every price in a set. Only the
amount is editable: a currency, a quantity band and a rule are what make a
price the price it is, and changing one is making a different price, which is
a different call.

An inventory item's screen counts its stock the same way — every location it
sits at, the count and what is incoming typed in place, written with one
`POST /admin/inventory-items/batch`. Reserved is not editable and will not
be: a reservation belongs to the order or cart holding the stock, and typing
over the number would release nothing.

Everywhere else, the round trip is what a shop uses.

The checkbox column is off unless a screen passes `select`, because a
checkbox on a list with no bulk action is a control that does nothing. A
selection is about the rows on screen: paging away drops it, rather than
leaving ids chosen that nobody can see for a bulk action to act on.

`features/batch/csv.ts` is sixty lines and no library. What bites in a shop's
data is a comma in a title, a quote in one, and a newline in a description;
RFC 4180 answers all three the same way and that is the whole of what is
there. Round-tripped against the real code — quotes, commas, an embedded
newline, CRLF from a spreadsheet, and a file that is only a header.

**Only the product's page has section editors.** A section that can be
changed has its own address and its own drawer — `/products/$id/organisation`
is one form, not a tab of a big one — and that is what keeps a save small
enough to describe: an operator who changed the origin country did not also
submit the title.

That was written here as impossible once, on the reasoning that the API has
one write route per record rather than one per part of it. The reasoning was
wrong: `PATCH /admin/products/{id}` takes every field as an `Option`, so a
form that sends three of them leaves the rest alone — which is what an
established platform's per-section drawers do too. Every other record's page still has one
editor, and for most of them that is right rather than unfinished: a customer
has six fields, a sales channel three, and splitting three fields across two
drawers is ceremony. The product is the record with seventeen.

**Mountable.** A host renders one element and gets the whole panel:

    import { Panel } from "tezgah-panel/src/panel"

    <Panel
      basepath="/admin/shop"
      apiBase="/api/commerce"
      token={() => session.accessToken}
      onUnauthenticated={() => session.signOut()}
      locale="tr"
    />

No screen it draws reaches for a global. Where the API is, what token to
send, what to do on a 401, which language to draw in and where these screens
live in the URL are all the host's answers, through `panel/runtime.ts`.

The router is built per mount rather than at module load, which is what makes
`basepath` possible at all and lets two panels sit on one origin. And
`bun run check:host` fails the build when anything outside the seven files
that *are* the standalone host reads `import.meta.env`, `localStorage` or
`document.cookie` — it caught one thing the day it was written: shadcn's
sidebar wrote `sidebar_state` at `path=/`, which a panel inside an
application would have written over that application's own with.

What is left is packaging. There is no library build here, so a host takes
the source — a published entry point is a build step and a version, not a
seam.

[`../../docs/architecture.md`](../../docs/architecture.md) carries all of this
beside the library and server gaps, with which layer owns each.

## What a host answers

`panel/runtime.ts` is everything this bundle needs from whoever is running
it, and nothing under `features/` reaches past it:

| | |
|---|---|
| `apiBase` | where `/admin/...` and `/store/...` are served |
| `token()` | the bearer token to send, read per request, or `null` for none |
| `onUnauthenticated()` | what to do when the host answers 401 |
| `locale` | `"en"` or `"tr"` |
| `basepath` | where the screens live in the host's URL, or `""` at the root |

`Panel` writes it and provides the locale to the tree, and
`src/panel/index.ts` is the whole of what a host may import — everything else
here is this repository's to move. `PanelProvider` is exported beside it for
a host composing the screens itself.
[`src/App.tsx`](src/App.tsx) is the standalone host's answers — the address
from `VITE_TEZGAH_API`, the token an operator pasted, and forgetting it on a
401 — and is about as small as that file should be. An application embedding
these screens writes its own, out of the session and the API base it already
has.

It is a module-level object that `PanelProvider` sets rather than a React
context, and that is deliberate: `api/mutator.ts`, the one function every
request goes through, is called from a query function outside any tree. A
hook cannot reach it. `configurePanel` is called during render rather than in
an effect for the same kind of reason — an effect runs after the first
render, and by then a screen's first query has already gone to whatever the
previous configuration named.

`panel/i18n` is a flat dictionary and a sixty-line lookup rather than
i18next: a panel that can be mounted inside somebody else's application must
not install a global singleton beside the one they already have. `en.ts`
holds every key, `tr.ts` is typed `Record<keyof typeof en, string>`, so a
string added in one language and not the other fails the build instead of
appearing untranslated on screen.

## What it talks to

[`../server`](../server) — the binary beside it, which mounts 116 of the 486
operations `api::routes()` declares. The crate itself serves no HTTP, so any
other host that mounts the same table will do; point the panel at whichever
one is running:

    VITE_TEZGAH_API=http://localhost:8080/api bun run dev

With nothing there, every screen says so rather than drawing an empty table —
and the same is true of a screen whose route that host has not bound, which
is most of the 228 this panel draws.

## Types

`tests/snapshots/openapi.json` declares 486 operations and, today, 22 of them
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

    bunx openapi-typescript ../../tests/snapshots/openapi.json -o src/api/schema.d.ts
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
operations each section's tag declares. Sixteen sections have screens —
products, orders, inventory, customers, promotions, subscriptions, store,
payouts, workflows, baskets, fulfilment, tax, pricing, payments, credit and
carts — which is 478 of the 486 operations. `digital` is the rest: its eight
operations exist and work, and `nav.ts`'s `folded` says why there is no
screen for them yet.

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
removing it. The sixteen built sections each have a real route; `digital`
falls through to `src/routes/$section.tsx`, which is what `NotBuilt` used to
be reached through — a static route always wins the match over a dynamic
one, so a slug that gains a screen just needs a file added here.

### A record's page is a stack of sections

`components/detail-page.tsx` is what all sixteen of them are built from:
loading, refusal and drift come from `QueryState`, going back comes from
`DetailHeader`, and what a screen decides is what the record is called, what
can be done to it, and which facts belong together. `main` is the record
itself, `side` is what it belongs to — one column on a narrow screen, in that
order.

Grouping is the whole work, and it is per record rather than mechanical. An
order's three statuses are one section because they move independently and
reading them together is the point. A subscription's dunning attempts sit
beside its status, because "retrying a failed charge" and "cancelled" are the
two an operator confuses. A tax rate's "default for its region" and
"combinable" belong together because the second only makes sense against the
first.

`Section`, `SectionRow`, `SectionRows` and `SectionBody` are the parts;
`ActionMenu` is what a section carries in its corner when there is something
to do to it. `MetadataSection` is the one every record has.

### A list is a framed container with a header

`DataTable` draws the frame, and `header` is the sentence saying what the
rows are. That matters most where there is no page title: `/tax/rates`,
`/fulfilment/providers`, `/payments/refund-reasons` and eleven more are tabs
under one heading, and without it each is a grid of rows and nothing that
says what they hold. `TableFrame` is the same frame for the three lists that
answer a plain array rather than `Page<T>` and so build their own table.

No feature is registered on TanStack Table, and there is no search box, no
filter and no sortable header. Every list handler in the crate takes a cursor
and a limit and nothing else, so all three would be claiming something about
the pages that are not on screen. It is a gap in the API, and
[`../../docs/architecture.md`](../../docs/architecture.md) carries it.

### ⌘K

`components/command-palette.tsx` searches the sections and nothing else. No
route searches products, orders or customers, so a palette offering to would
promise something the API cannot answer; when one arrives, that is where it
goes.

Reaching a section from a runtime slug takes a switch, because the router's
route union is closed and a template string cannot join it.
`components/section-link.tsx` is that switch, once, for the sidebar and the
palette both.

### A form is a route, drawn over the page it came from

A creation or edit form is still an address — `/products/new`,
`/products/$id/edit` are links somebody can send, and the back button works —
but it is drawn *over* the page it was opened from rather than replacing it.
An operator who opens a form has not lost the page of products they were
looking at, and cancelling puts them back on it with the cursor they had
rather than on page one.

That makes it a **child** route, not a sibling: `routes/products.new.tsx` is
`/products/new` nested under `routes/products.tsx`, which renders the list
and an `<Outlet />` beneath it. `components/modals/` is what draws into that
outlet:

- `RouteFocusModal` — a form that wants the screen. Creating a product, and
  later importing a file. Near-fullscreen rather than a small dialog, because
  a creation form with six sections in a 400-pixel box is a scroll bar with a
  title.
- `RouteDrawer` — a form that changes one part of a record it is standing on.
  A drawer, so the record stays readable behind it.
- `RouteModalProvider` — the one thing that knows how to leave. `close()` is
  what the close button, the backdrop and escape all end at; `succeed()` is
  the same thing after a write, and `markSaved()` is its half for a form that
  saves and then goes somewhere the modal does not know about, like a
  creation form landing on the record it just made.
- `RouteModalForm` — react-hook-form plus the unsaved-changes guard. The
  guard is on *navigation* rather than on the modal's own close, because
  every way out of a route modal is a navigation: the close button, the
  backdrop, escape, the browser's back button, a link inside the form.

`components/form/form.tsx`'s `FormField` is the other half — a `Controller`
over the zod schema in `api/schemas.ts`, so validation, dirty state and field
errors come from the schema rather than from a `useState` and a hand-rolled
`Record<string, string>` in each screen. `features/products/` is converted;
the four remaining `*-edit.tsx` screens are drawn in a `RouteDrawer` and
still hand-roll their form.

#### The outlet is not optional, and its absence is silent

`/products/$id/edit` is a child of `/products/$id`. Until #250 that page
rendered no `<Outlet />`, so the address resolved, the product page drew, and
the edit form was never rendered at all. Every one of the panel's five "edit
a record" screens was in that state — products, customers, promotions, store
regions and sales channels — from the commit that added them.

Nothing catches this: the route file is right, the screen is right, and the
router reports a match. What finds it is reading `routeTree.gen.ts` for
routes with children and checking that each of those components draws an
outlet.

#### `/store/keys/new` is still the exception to "save returns to the list"

The token it mints is shown once and stored nowhere it could be read back, so
that screen shows it in place first and only returns to `/store/keys` once
the operator says "Done". Navigating away immediately would make the token
unreachable before a screen ever got to show it.

### A row is a route too

`/products/$id`, `/orders/$id`, `/inventory/$id`, `/customers/$id`,
`/promotions/$id`, `/subscriptions/$id`, `/store/regions/$id`,
`/store/sales-channels/$id`, `/workflows/$id`, `/fulfilment/shipping-options/$id`,
`/fulfilment/shipping-profiles/$id`, `/tax/rates/$id`, `/tax/regions/$id`,
`/pricing/price-lists/$id`, `/payments/$id` and `/credit/$id` are the sixteen
`GET .../{id}` operations bound alongside their lists — `src/features/<domain>/detail.tsx`
or `.../<domain>-detail.tsx`, one screen each, reading every field the
response carries rather than the subset its list column happened to need. A
list whose `GET .../{id}` is not bound — `fulfilment`'s providers, sets and
shipping option types; `tax`'s registrations; `payment`'s refund reasons;
`carts` entirely — draws no `rowLink`, because there is nowhere for the row
to go. Their route files take the
trailing-underscore escape (`routes/products_.$id.tsx`,
`routes/store_.regions.$id.tsx`) — a record's page is a page of its own, not
a form drawn over the list, and `/store`'s tabs are chrome it does not want.
Its *own* children, `/products/$id/edit` and the rest, are nested under it in
the ordinary way, which is what the outlet above is for. `components/data-table.tsx`'s `rowLink` is what gets a table row there:
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

`/fulfilment` (five tabs), `/tax` (three) and `/pricing` (four) take the
`/store` shape — none of their tabs is the section itself, so each redirects
from its own index to the first. `/payments` takes the `/payouts` shape
instead: `payments` is real content on its own, so `routes/payments.index.tsx`
renders it directly and only `/payments/refund-reasons` is a sibling route.
`pricing`'s own two operations without a list — `price-sets` (only
`GET .../{id}`) and `prices` (only listed scoped to the set that owns them,
`GET /admin/price-sets/{id}/prices`) — are lookups by id on their own tab, the
same shape as `/payouts`'s balance and order-lines widgets, not tables with
nothing to page through. `price-preferences` is the same again: `GET
/admin/price-preferences` wants an `attribute` the document does not declare
as a parameter, so it is reached the same undeclared way `/admin/payout-balance/{currency_code}`
already was.

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

### Carts have a list and no detail

`GET /admin/carts` is bound and was, until now, reachable from nothing — the
back office drew no screen for it, so `tests/reachable.rs` tolerated it under
a reason that named exactly that. `/carts` (`src/features/carts/screen.tsx`)
is what makes the reason false. There is still no `GET /admin/carts/{id}` —
only a shopper's own cart is fetched by id, at `/store/carts/{id}` — so
`/carts`'s rows are not `Link`s.

A handful of other lists are `Page<T>`-shaped in the document but orval does
not export their item on its own — `fulfilment`'s sets and shipping option
types, `payment`'s refund reasons — because the schema embeds the item inline
rather than by `$ref`. `api/schemas.ts` transcribes those by hand, the same
discipline as an operation #202 does not document at all, and says so at each
one. A few lists answer a plain array rather than `Page<T>` and so skip
`usePagedList` for a small `useQuery` of their own, the same as
`features/store/currencies.tsx` already did: `fulfilment`'s providers and
`tax`'s registrations.

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
