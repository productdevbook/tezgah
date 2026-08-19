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
type names the Rust struct it mirrors. That is not the intended state; until
the generator emits schemas these drift silently.

Regenerate the path set with:

    bunx openapi-typescript ../tests/snapshots/openapi.json -o src/api/schema.d.ts

CI fails if it is stale.

## Coverage

The sidebar reads `src/lib/nav.ts`, which carries the number of admin
operations each section's tag declares. Three sections have screens — products,
orders, inventory. The rest say how many operations they are not drawing yet,
because the operations exist and work; only the screen is missing.

## Two of shadcn's own files are patched

`src/components/ui/spinner.tsx` and `scroll-area.tsx` do not typecheck as
generated — an svg's `strokeWidth` is `string | number` where `HugeiconsIcon`
wants a number, and a namespace import nothing reads. Both are fixed in place,
and `shadcn add` will write them back the way they were. If a build starts
failing on either, that is what happened.

## Scripts

    bun run dev        # vite
    bun run build      # tsc -b && vite build
    bun run lint
    bun run typecheck
