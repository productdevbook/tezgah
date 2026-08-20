/** A record's fields, laid out so every one of them stays visible — nothing here decides what a screen hides. */
export function FieldGrid({ children }: { children: React.ReactNode }) {
  return <dl className="grid grid-cols-1 gap-x-8 gap-y-4 sm:grid-cols-2">{children}</dl>
}

export function DetailField({
  label,
  full,
  children,
}: {
  label: string
  full?: boolean
  children: React.ReactNode
}) {
  return (
    <div className={full ? "sm:col-span-2" : undefined}>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 text-sm break-words">{children}</dd>
    </div>
  )
}

export function Empty() {
  return <span className="text-muted-foreground">—</span>
}

/** `serde_json::Value`, shown rather than summarised — empty and absent read the same. */
export function Metadata({ value }: { value: unknown }) {
  const isEmptyObject =
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value as object).length === 0
  if (value === null || value === undefined || isEmptyObject) return <Empty />
  return (
    <pre className="max-w-full overflow-x-auto rounded-md bg-muted p-2 text-xs">
      {JSON.stringify(value, null, 2)}
    </pre>
  )
}
