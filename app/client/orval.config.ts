import { defineConfig } from "orval"

import transformer from "./orval/transformer.cjs"

/**
 * The document declares 486 operations against 34 documented schemas
 * (productdevbook/tezgah#202 is the rest). Orval reads every path either
 * way — the path set is what stopped a typo becoming a request nobody
 * answers before this existed — but only a documented operation gets a
 * generated body type; everywhere else the caller still writes zod by hand
 * against the Rust struct, same as before.
 */
const input = {
  target: "../../tests/snapshots/openapi.json",
  override: { transformer },
} as const

export default defineConfig({
  fetch: {
    input,
    output: {
      target: "./src/api/generated/fetch",
      schemas: "./src/api/generated/fetch/models",
      mode: "tags-split",
      client: "fetch",
      httpClient: "fetch",
      clean: true,
      indexFiles: true,
      override: {
        mutator: { path: "./src/api/mutator.ts", name: "apiMutator" },
      },
    },
  },
  zod: {
    input,
    output: {
      target: "./src/api/generated/zod",
      mode: "tags-split",
      client: "zod",
      clean: true,
      indexFiles: true,
    },
  },
})
