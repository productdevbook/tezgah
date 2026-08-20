import { createContext, useContext } from "react"

import { en, type TranslationKey } from "@/panel/i18n/en"
import { tr } from "@/panel/i18n/tr"

export type { TranslationKey }

export const LOCALES = { en, tr } as const

export type Locale = keyof typeof LOCALES

/**
 * A dictionary and a lookup, rather than i18next.
 *
 * A mountable panel must not install a global singleton: a host that already
 * translates its own screens has an i18n instance of its own, and two of them
 * fighting over one `<html lang>` is worse than the sixty lines this costs.
 * `{name}` is the only interpolation, because that is the only one used.
 */
export function translate(
  locale: Locale,
  key: TranslationKey,
  vars?: Record<string, string | number>
): string {
  const text = LOCALES[locale][key] ?? en[key]
  if (!vars) return text
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole
  )
}

export const LocaleContext = createContext<Locale>("en")

export function useT() {
  const locale = useContext(LocaleContext)
  return (key: TranslationKey, vars?: Record<string, string | number>) =>
    translate(locale, key, vars)
}

export function useLocale(): Locale {
  return useContext(LocaleContext)
}
