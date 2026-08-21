import { Link } from "@tanstack/react-router"
import { useT } from "@/panel/i18n"

import { taxRate, type TaxRate } from "@/api/schemas"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Empty } from "@/components/detail-fields"
import { usePagedList } from "@/lib/paged"
import { SearchBox } from "@/components/search-box"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"

const columns: Columns<TaxRate> = [
  {
    header: "field.name",
    accessorKey: "name",
    meta: { className: "font-medium" },
  },
  {
    header: "field.code",
    accessorKey: "code",
    cell: ({ row }) => row.original.code ?? <Empty />,
    meta: { className: "font-mono text-xs" },
  },
  {
    header: "field.rate",
    accessorKey: "rate",
    meta: { className: "text-right font-mono text-xs" },
  },
  {
    header: "field.combinable",
    accessorKey: "is_combinable",
    cell: ({ row }) =>
      row.original.is_combinable ? (
        <Badge variant="outline">combinable</Badge>
      ) : null,
  },
  {
    header: "field.default",
    accessorKey: "is_default",
    cell: ({ row }) =>
      row.original.is_default ? <Badge>default</Badge> : null,
    meta: { className: "text-right" },
  },
]

export function TaxRates({
  after,
  q,
  kind,
  onAfterChange,
  onQChange,
  onKindChange,
}: {
  after: string | undefined
  q: string | undefined
  kind: "all" | "default" | "combinable"
  onAfterChange: (after: string | undefined) => void
  onQChange: (q: string | undefined) => void
  onKindChange: (kind: "all" | "default" | "combinable") => void
}) {
  const t = useT()
  const paged = usePagedList(
    ["tax-rates", q ?? "", kind],
    "/admin/tax-rates",
    taxRate,
    {
      after,
      onAfterChange,
      query: {
        q,
        default: kind === "default" ? "true" : undefined,
        combinable: kind === "combinable" ? "true" : undefined,
        count: "true",
      },
    }
  )

  return (
    <DataTable
      header={{
        title: t("frame.taxRates"),
        description: t("frame.taxRatesWhy"),
        actions: (
          <div className="flex items-center gap-2">
            <SearchBox
              value={q}
              onChange={onQChange}
              placeholder={t("search.taxRates")}
            />
            <Select
              value={kind}
              onValueChange={(value) =>
                onKindChange(value as "all" | "default" | "combinable")
              }
            >
              <SelectTrigger className="w-40" size="sm">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">{t("filter.anyRate")}</SelectItem>
                <SelectItem value="default">
                  {t("filter.theDefault")}
                </SelectItem>
                <SelectItem value="combinable">
                  {t("filter.stacking")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        ),
      }}
      paged={paged}
      columns={columns}
      rowLink={(row) => (
        <Link
          to="/tax/rates/$id"
          params={{ id: row.id }}
          className="absolute inset-0"
          aria-label={`Open ${row.name}`}
        />
      )}
      empty={{
        title: t("empty.taxRates"),
        description: t("empty.taxRatesWhy"),
      }}
    />
  )
}
