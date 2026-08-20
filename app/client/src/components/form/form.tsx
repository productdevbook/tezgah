import {
  Controller,
  type Control,
  type ControllerRenderProps,
  type FieldPath,
  type FieldValues,
} from "react-hook-form"
import type { ReactNode } from "react"

import {
  Field,
  FieldDescription,
  FieldError,
  FieldLabel,
} from "@/components/ui/field"

/**
 * One field, wired to the form holding it.
 *
 * The panel's write schemas are already zod — generated where the document
 * has one, transcribed from the Rust struct where it does not — so a form is
 * `useForm({ resolver: zodResolver(schema) })` and this is what puts a field
 * of it on screen. Nothing here validates: the schema does, in one place, and
 * this draws what it said.
 */
export function FormField<
  TValues extends FieldValues,
  TName extends FieldPath<TValues>,
>({
  control,
  name,
  label,
  description,
  children,
}: {
  control: Control<TValues>
  name: TName
  label: string
  description?: string
  children: (field: ControllerRenderProps<TValues, TName>) => ReactNode
}) {
  return (
    <Controller
      control={control}
      name={name}
      render={({ field, fieldState }) => (
        <Field data-invalid={fieldState.error ? true : undefined}>
          <FieldLabel htmlFor={field.name}>{label}</FieldLabel>
          {children(field)}
          {description ? (
            <FieldDescription>{description}</FieldDescription>
          ) : null}
          <FieldError errors={fieldState.error ? [fieldState.error] : []} />
        </Field>
      )}
    />
  )
}
