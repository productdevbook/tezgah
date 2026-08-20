import js from '@eslint/js'
import globals from 'globals'
import reactHooks from 'eslint-plugin-react-hooks'
import reactRefresh from 'eslint-plugin-react-refresh'
import tseslint from 'typescript-eslint'
import { defineConfig, globalIgnores } from 'eslint/config'

export default defineConfig([
  globalIgnores(['dist']),
  {
    files: ['**/*.{ts,tsx}'],
    extends: [
      js.configs.recommended,
      tseslint.configs.recommended,
      reactHooks.configs.flat.recommended,
      reactRefresh.configs.vite,
    ],
    languageOptions: {
      globals: globals.browser,
    },
  },
  // After the block above, because the last matching config wins.
  //
  // shadcn writes these files and `shadcn add` overwrites them, so a fix made
  // in one is erased the next time a component is added. They break both rules
  // by design: a component file that also exports its `cva` variants, and a
  // hook that sets state from an effect.
  {
    files: ['src/components/ui/**', 'src/hooks/use-mobile.ts'],
    rules: {
      'react-refresh/only-export-components': 'off',
      'react-hooks/set-state-in-effect': 'off',
    },
  },
  // Every file here exports `Route` — `@tanstack/router-plugin`'s own
  // convention, not a component. Its own component is `RouteComponent`,
  // exported alongside it (not inlined) because a hook may only be called
  // from something react-hooks recognises as a component, and an unexported
  // one still has to be named like it calls hooks.
  {
    files: ['src/routes/**'],
    rules: {
      'react-refresh/only-export-components': ['error', { allowExportNames: ['Route'] }],
    },
  },
])
