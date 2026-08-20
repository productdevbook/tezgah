// A child route whose parent draws no `<Outlet />` is a screen nothing can
// reach: the address resolves, the parent renders, and the child never
// appears. Five of the panel's screens were in that state from the commit
// that added them — the route file was right, the screen was right, and the
// router reported a match.
//
// Nothing in the type system or the router says so, hence this. It reads the
// generated route tree rather than a list kept by hand, so a parent gained
// tomorrow is checked tomorrow.
import { readFileSync } from "node:fs"
import { readdirSync } from "node:fs"
import { join } from "node:path"

const src = new URL("../src/", import.meta.url).pathname
const tree = readFileSync(join(src, "routeTree.gen.ts"), "utf8")

/** `interface FooRouteChildren {` — the generator writes one per parent. */
const parents = [...tree.matchAll(/interface (\w+)RouteChildren \{/g)].map(
  (match) => match[1]
)

/**
 * `import { Route as FooRouteImport } from './routes/bar'`, plus the root,
 * which the generator writes in lower case and names `RootRouteChildren`.
 */
const files = new Map(
  [...tree.matchAll(/import \{ Route as (\w+)RouteImport \} from '\.\/(.+)'/g)].map(
    (match) => [match[1], match[2]]
  )
)
files.set("Root", files.get("root") ?? "routes/__root")

const routes = readdirSync(join(src, "routes"))

/**
 * A doc comment explaining why the outlet is there reads exactly like the
 * outlet, so this searched its own explanation and passed a file that had
 * none. Comments come out first.
 */
function code(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/[^\n]*/g, "")
}

function resolve(specifier) {
  if (specifier.startsWith("@/")) return join(src, specifier.slice(2))
  return null
}

/**
 * The route file, plus every local module it imports. A route is allowed to
 * delegate its outlet to a layout component — `routes/store.tsx` renders
 * `StoreLayout`, and the outlet is in there — so one hop is followed.
 */
function drawsOutlet(file) {
  const path = join(src, `${file}.tsx`)
  const source = readFileSync(path, "utf8")
  if (code(source).includes("<Outlet")) return true

  for (const [, specifier] of source.matchAll(/from "([^"]+)"/g)) {
    const local = resolve(specifier)
    if (!local) continue
    try {
      if (code(readFileSync(`${local}.tsx`, "utf8")).includes("<Outlet")) return true
    } catch {
      // not a .tsx module; nothing to read for an outlet
    }
  }
  return false
}

const missing = []
for (const parent of parents) {
  const file = files.get(parent)
  if (!file) {
    console.error(`no import found for ${parent}RouteImport`)
    process.exit(1)
  }
  if (!drawsOutlet(file)) missing.push(file)
}

if (missing.length > 0) {
  console.error(
    "these routes have child routes and draw no <Outlet />, so every child is unreachable:"
  )
  for (const file of missing) console.error(`  src/${file}.tsx`)
  process.exit(1)
}

console.log(
  `${parents.length} routes have children, and all of them draw an outlet` +
    ` (${routes.length} route files)`
)
