// The panel may be mounted inside somebody else's application. Anything that
// reaches for a browser global decides something for that application: which
// API to call, what to keep in its storage, what cookie to write on its
// origin. Those are the host's answers, and `panel/runtime.ts` is where a
// host gives them.
//
// So only the files that *are* the standalone host may read one. It caught
// exactly one thing when it was written: shadcn's sidebar wrote
// `sidebar_state` at `path=/`, which a panel mounted inside an application
// would have written over that application's own with.
//
// `document.cookie` is here and the rest of `document` is not: reading the
// DOM is what a component does, while writing a cookie is keeping something
// on an origin that is not yours.

import { readdirSync, readFileSync, statSync } from "node:fs"
import { join } from "node:path"

const ROOT = new URL("../src", import.meta.url).pathname

// Each of these is the standalone panel answering for itself, which is
// exactly what a host is supposed to do.
const HOSTS = new Set([
  "App.tsx", // the standalone host: its API address and its token
  "main.tsx", // mounts it on a page it owns
  "lib/token.ts", // that host's idea of where a token is kept
  "components/connect.tsx", // and how one is obtained
  "components/theme-provider.tsx", // the standalone page's own theme
  "panel/runtime.ts", // where a host's answers are read from
  "panel/index.ts", // says the rule in prose; the prose contains the words
])

const REACHES = [
  ["import.meta.env", /import\.meta\.env/],
  ["localStorage", /\blocalStorage\b/],
  ["sessionStorage", /\bsessionStorage\b/],
  ["document.cookie", /\bdocument\.cookie\b/],
]

function* files(dir, prefix = "") {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry)
    const at = prefix ? `${prefix}/${entry}` : entry
    if (statSync(path).isDirectory()) {
      // Generated from the OpenAPI document; not ours to write rules about.
      if (at === "api/generated") continue
      yield* files(path, at)
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      yield [at, path]
    }
  }
}

const found = []
let checked = 0

for (const [at, path] of files(ROOT)) {
  if (HOSTS.has(at)) continue
  checked += 1
  const text = readFileSync(path, "utf8")
  for (const [name, pattern] of REACHES) {
    if (pattern.test(text)) found.push(`${at}: ${name}`)
  }
}

if (found.length > 0) {
  console.error(
    `${found.length} file(s) reach for something that belongs to whoever is ` +
      `running the panel:\n` +
      found.map((one) => `  ${one}`).join("\n") +
      `\n\nAsk panel/runtime.ts for it instead, or — if this file really is ` +
      `the standalone host answering for itself — add it to HOSTS in ` +
      `scripts/check-host-answers.mjs with the reason.`
  )
  process.exit(1)
}

console.log(
  `${checked} files checked, and none of them decides something the host ` +
    `should (${HOSTS.size} are the host).`
)
