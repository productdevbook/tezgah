import { useEffect, useState } from "react"

/** ⌘K on a Mac, Ctrl+K everywhere else. */
export function useCommandPalette() {
  const [open, setOpen] = useState(false)

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key.toLowerCase() !== "k") return
      if (!event.metaKey && !event.ctrlKey) return
      event.preventDefault()
      setOpen((was) => !was)
    }
    document.addEventListener("keydown", onKeyDown)
    return () => document.removeEventListener("keydown", onKeyDown)
  }, [])

  return { open, setOpen }
}
