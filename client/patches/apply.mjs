// Re-applies the two fixes shadcn's generator undoes. Idempotent by design:
// it reports "already right" rather than failing, so it can run on every build.
import { readFileSync, writeFileSync } from "node:fs"

const patches = [
  {
    file: "src/components/ui/spinner.tsx",
    from: 'function Spinner({ className, ...props }: React.ComponentProps<"svg">) {',
    to: 'function Spinner({\n  className,\n  ...props\n}: Omit<React.ComponentProps<"svg">, "strokeWidth">) {',
    why: "HugeiconsIcon wants a numeric strokeWidth; an svg's is string | number",
  },
  {
    file: "src/components/ui/scroll-area.tsx",
    from: 'import * as React from "react"\n',
    to: "",
    why: "the React namespace is imported and never read",
  },
]

let changed = 0
for (const patch of patches) {
  const source = readFileSync(patch.file, "utf8")
  if (!source.includes(patch.from)) {
    console.log(`  already right: ${patch.file}`)
    continue
  }
  writeFileSync(patch.file, source.replace(patch.from, patch.to))
  console.log(`  patched: ${patch.file} — ${patch.why}`)
  changed++
}
console.log(changed ? `${changed} patched` : "nothing to patch")
