// Runs the same generator `@tanstack/router-plugin`'s Vite plugin calls, but
// stand-alone: `routeTree.gen.ts` has to exist before `tsc -b` (the first
// step of both `build` and `typecheck`) ever asks Vite to run a plugin, so it
// is committed rather than produced by the build itself.
import { Generator, getConfig } from "@tanstack/router-generator"

const root = new URL("..", import.meta.url).pathname
const config = getConfig({ target: "react", autoCodeSplitting: true }, root)
const generator = new Generator({ config, root })
await generator.run()
console.log(`routeTree.gen.ts written to ${config.generatedRouteTree}`)
