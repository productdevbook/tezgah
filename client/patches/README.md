# patches

Two of shadcn's own components do not typecheck as generated, and
`shadcn add` writes them back the way they were every time. They have been
re-applied by hand twice now — once at `base-luma`, once at `base-maia` — so
they live here instead.

    bun run patch

`patch` is idempotent: it checks whether the fix is already in the file and
does nothing if it is. It runs as part of `build`, so a forgotten re-apply is
a failing typecheck rather than a surprise.

| File | What is wrong as generated |
|---|---|
| `spinner.tsx` | spreads `React.ComponentProps<"svg">` into `HugeiconsIcon`, whose `strokeWidth` is `number` where an svg's is `string \| number` |
| `scroll-area.tsx` | imports the `React` namespace and never reads it, which `noUnusedLocals` refuses |

If a future preset fixes either upstream, `patch` will say the file already
looks right and this directory can lose an entry.
