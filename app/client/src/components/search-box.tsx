import { Search01Icon } from "@hugeicons/core-free-icons"
import { HugeiconsIcon } from "@hugeicons/react"
import { useEffect, useState } from "react"

import { Input } from "@/components/ui/input"

/**
 * A search box over a list whose address carries what was typed.
 *
 * Two states on purpose. What is in the input is this component's, so typing
 * is not one request per keystroke; what is in the URL is the route's, and it
 * is what the list is actually showing — so `/products?q=denim` is a page
 * somebody else can open. The delay between them is the debounce, and it is
 * the only thing this component decides.
 *
 * It resets the cursor by way of the route, not here: a cursor names a row in
 * the ordering it was issued under and means nothing under another filter.
 */
export function SearchBox({
  value,
  onChange,
  placeholder,
}: {
  value: string | undefined
  onChange: (next: string | undefined) => void
  placeholder: string
}) {
  const [typed, setTyped] = useState(value ?? "")
  const [showing, setShowing] = useState(value)

  // The address is the truth: a back button, or a link somebody opened, has to
  // land in the box as well as in the list. Adjusted during render rather than
  // in an effect — React's own answer for a prop the state has to follow, and
  // it re-renders before anything is painted rather than after.
  if (value !== showing) {
    setShowing(value)
    setTyped(value ?? "")
  }

  useEffect(() => {
    const trimmed = typed.trim()
    const next = trimmed === "" ? undefined : trimmed
    if (next === (value ?? undefined)) return

    const timer = setTimeout(() => onChange(next), 300)
    return () => clearTimeout(timer)
  }, [typed, value, onChange])

  return (
    <div className="relative w-full sm:w-64">
      <HugeiconsIcon
        icon={Search01Icon}
        strokeWidth={2}
        className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
      />
      <Input
        type="search"
        className="pl-9"
        value={typed}
        placeholder={placeholder}
        aria-label={placeholder}
        onChange={(event) => setTyped(event.target.value)}
      />
    </div>
  )
}
