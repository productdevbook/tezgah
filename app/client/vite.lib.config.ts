import path from "path"
import tailwindcss from "@tailwindcss/vite"
import { tanstackRouter } from "@tanstack/router-plugin/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

/**
 * The panel as something a host can install, rather than a site to deploy.
 *
 * `vite.config.ts` builds the standalone application — an `index.html`, a
 * bundle, a set of assets. This builds the other consumer `panel/index.ts`
 * was written for: an application that already has a back office and wants
 * these screens inside it.
 *
 * React is external and everything else is bundled. A host has React —
 * two copies in one page is the hooks error nobody can read — and the rest
 * is this package's business: a host should not have to know that the panel
 * uses TanStack Router or Base UI, and having to install them to render one
 * component would make the seam a lie.
 *
 * The stylesheet comes out beside the JavaScript rather than being injected,
 * so a host decides when it loads and can put its own cascade either side of
 * it.
 */
export default defineConfig({
  plugins: [
    tanstackRouter({ target: "react", autoCodeSplitting: false }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: { "@": path.resolve(__dirname, "./src") },
  },
  build: {
    outDir: "dist-lib",
    emptyOutDir: true,
    // No code splitting: a host bundles this again, and chunks it cannot see
    // the graph of are chunks it cannot inline.
    cssCodeSplit: false,
    lib: {
      entry: path.resolve(__dirname, "src/panel/index.ts"),
      formats: ["es"],
      fileName: () => "panel.js",
    },
    rollupOptions: {
      external: ["react", "react-dom", "react/jsx-runtime", "react-dom/client"],
      output: {
        assetFileNames: (asset) =>
          asset.names?.some((name) => name.endsWith(".css"))
            ? "panel.css"
            : "[name][extname]",
      },
    },
  },
})
