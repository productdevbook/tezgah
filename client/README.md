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
operations each section's tag declares. Seven sections have screens — products,
orders, inventory, customers, promotions, subscriptions and store — which is
330 of the 483 operations. The rest say how many they are not drawing yet,
because those operations exist and work; only the screen is missing.

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
in `src/features/<domain>/screen.tsx` and never imports from `@tanstack/router`
itself; a route owns the URL, a screen just takes what it's given. The seven
built sections each have a real route; the other ten fall through to
`src/routes/$section.tsx`, which is what `NotBuilt` used to be reached
through — a static route always wins the match over a dynamic one, so a slug
that gains a screen just needs a file added here.

### Pagination lives in the URL

Every paginated route's `validateSearch` carries `after` (products also
`status`), so the page a screen shows is always the one named in its own
address — `/products?status=draft&after=<cursor>` is a link somebody else can
open to the same page. `src/lib/paged.ts`'s `usePagedList` still keeps a
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
