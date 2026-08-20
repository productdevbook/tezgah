import { z } from "zod"

/**
 * What a product form collects, as opposed to what the API takes.
 *
 * Not the wire schema: `createProduct` and `updateProduct` in `api/schemas.ts`
 * are still the only thing a request is built from, and this is converted to
 * one on submit. They differ where a form has to — a text input holds `""`
 * and never `undefined`, and the API wants the field absent rather than
 * empty — and keeping that conversion in one place is what stops a blank
 * subtitle being saved as the two-character string it looks like.
 */
export const productFields = z.object({
  handle: z
    .string()
    .trim()
    .min(1, "a handle is needed")
    .regex(/^\S+$/, "a handle has no spaces in it"),
  title: z.string().trim().min(1, "a title is needed"),
  subtitle: z.string().trim(),
  description: z.string().trim(),
})

export type ProductFields = z.infer<typeof productFields>

export const EMPTY_PRODUCT: ProductFields = {
  handle: "",
  title: "",
  subtitle: "",
  description: "",
}

/** `""` is what an untouched input holds; the API wants the field left out. */
export function orAbsent(value: string): string | undefined {
  return value.trim() === "" ? undefined : value
}

/** The same, for `PATCH`, where clearing a field is `null` rather than absence. */
export function orNull(value: string): string | null {
  return value.trim() === "" ? null : value
}
