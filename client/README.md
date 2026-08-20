# client

tezgah's admin panel. React 19, Vite, TanStack Router and Query, shadcn.

## What it talks to

Nothing, yet. tezgah is a library and serves no HTTP itself — something has to
mount `api::routes()` first, which is what the repository's issue #199 is
about. Point the panel at whatever does:

    VITE_TEZGAH_API=http://localhost:8080/api bun run dev

With nothing there, every screen says so rather than drawing an empty table.

## Types

`src/api/schema.d.ts` is generated from `../tests/snapshots/openapi.json` and
gives one thing: the set of paths that exist, so a typo cannot become a request
nobody answers. It cannot give more — the document declares 483 operations and
**zero schemas**: no request bodies, no response bodies, no
`components/schemas`.

So `src/api/views.ts` is transcribed from `src/api/*.rs` **by hand**, and each
schema names the Rust struct it mirrors.

They are zod schemas rather than plain types, and parsed at the boundary, for
exactly that reason. A hand-written type drifts in silence — a renamed field
becomes `undefined` in a cell and nobody learns anything. Parsed, the same
drift is its own error kind, `drifted`, and the screen says which field it
happened to. The cost is one parse per response; what it buys is that this file
cannot quietly become fiction while #202 is open.

Regenerate the path set with:

    bunx openapi-typescript ../tests/snapshots/openapi.json -o src/api/schema.d.ts

CI fails if it is stale.

## Coverage

The sidebar reads `src/lib/nav.ts`, which carries the number of admin
operations each section's tag declares. Seven sections have screens — products,
orders, inventory, customers, promotions, subscriptions and store — which is
330 of the 483 operations. The rest say how many they are not drawing yet,
because those operations exist and work; only the screen is missing.

Tables are TanStack Table v9, with no feature registered. Nothing sorts or
filters on the client on purpose: every list handler takes a cursor and a limit
and nothing else, so a sortable header would be claiming something about the
pages that are not on screen.

## Two of shadcn's own files are patched

`src/components/ui/spinner.tsx` and `scroll-area.tsx` do not typecheck as
generated, and every `shadcn add` or preset change writes them back the way
they were. That happened twice — once at `base-luma`, once at `base-maia` — so
the fixes are in [`patches/`](patches) and `bun run patch` re-applies them.
It runs as part of `build` and `typecheck`, and is idempotent, so a forgotten
re-apply is a failing typecheck rather than a surprise.

## Scripts

    bun run dev        # vite
    bun run build      # tsc -b && vite build
    bun run lint
    bun run typecheck
