/**
 * The smallest CSV that survives a spreadsheet, and no library.
 *
 * What actually bites in a shop's data is three things: a comma in a product
 * title, a quote in one, and a newline in a description. RFC 4180 answers all
 * three the same way — wrap the field in quotes and double any quote inside
 * it — and that is the whole of what is here.
 *
 * What is deliberately not here: character encodings, a delimiter that is not
 * a comma, and a header row in another order. A shop that needs those has a
 * spreadsheet that can save what this reads.
 */
export function toCsv(header: readonly string[], rows: string[][]): string {
  const escape = (field: string) =>
    /[",\n\r]/.test(field) ? `"${field.replace(/"/g, '""')}"` : field

  return [header, ...rows].map((row) => row.map(escape).join(",")).join("\r\n")
}

/**
 * Reads back what `toCsv` writes, and what a spreadsheet saves.
 *
 * Returns the header separately, because a row is only meaningful against it
 * — a file whose columns are in another order is still readable, and a file
 * missing one is the caller's to refuse.
 */
export function fromCsv(text: string): { header: string[]; rows: string[][] } {
  const rows: string[][] = []
  let row: string[] = []
  let field = ""
  let quoted = false
  let i = 0

  const endField = () => {
    row.push(field)
    field = ""
  }
  const endRow = () => {
    endField()
    // A trailing newline is a line ending, not an empty row.
    if (row.length > 1 || row[0] !== "") rows.push(row)
    row = []
  }

  while (i < text.length) {
    const character = text[i]

    if (quoted) {
      if (character === '"') {
        if (text[i + 1] === '"') {
          field += '"'
          i += 2
          continue
        }
        quoted = false
        i += 1
        continue
      }
      field += character
      i += 1
      continue
    }

    if (character === '"' && field === "") {
      quoted = true
      i += 1
      continue
    }
    if (character === ",") {
      endField()
      i += 1
      continue
    }
    if (character === "\r" || character === "\n") {
      endRow()
      // \r\n is one ending.
      i += text[i] === "\r" && text[i + 1] === "\n" ? 2 : 1
      continue
    }

    field += character
    i += 1
  }

  if (field !== "" || row.length > 0) endRow()

  const header = rows.shift() ?? []
  return { header, rows }
}
