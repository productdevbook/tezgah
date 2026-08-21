import { useQuery } from "@tanstack/react-query"

import { get } from "@/api/client"
import { page, variant } from "@/api/schemas"
import { Empty, Mono } from "@/components/detail-fields"
import { Section } from "@/components/section"
import { Badge } from "@/components/ui/badge"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { useT } from "@/panel/i18n"

/**
 * What a shop actually sells.
 *
 * A product is a name and a description; the variant is the row with the
 * SKU, the price set and the stock behind it, and a product screen without
 * them describes something nobody can buy.
 *
 * A row goes nowhere: there is no `GET /admin/product-variants/{id}`, so
 * there is nothing to open. What can be reached from a variant today is its
 * bundle and its digital content, both of which have screens of their own.
 */
export function Variants({ productId }: { productId: string }) {
  const t = useT()
  const result = useQuery({
    queryKey: ["product-variants", productId],
    queryFn: ({ signal }) =>
      get("/admin/products/{id}/variants", {
        signal,
        schema: page(variant),
        params: { id: productId },
        query: { limit: 100 },
      }),
  })

  const rows = result.data?.items ?? []

  return (
    <Section
      title={t("section.variants")}
      description={t("section.variantsWhy")}
    >
      {result.isPending ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">Loading…</p>
      ) : rows.length === 0 ? (
        <p className="px-6 py-4 text-sm text-muted-foreground">
          No variants. Nothing on this product is for sale until one exists.
        </p>
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Title</TableHead>
              <TableHead>SKU</TableHead>
              <TableHead>Stock</TableHead>
              <TableHead className="text-right">Rank</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => (
              <TableRow key={row.id}>
                <TableCell className="font-medium">{row.title}</TableCell>
                <TableCell>
                  {row.sku ? <Mono>{row.sku}</Mono> : <Empty />}
                </TableCell>
                <TableCell>
                  {row.manages_inventory ? (
                    <div className="flex gap-1">
                      <Badge variant="default">counted</Badge>
                      {row.allows_backorder ? (
                        <Badge variant="outline">backorder</Badge>
                      ) : null}
                    </div>
                  ) : (
                    <Badge variant="outline">not counted</Badge>
                  )}
                </TableCell>
                <TableCell className="text-right font-mono text-xs text-muted-foreground">
                  {row.rank}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Section>
  )
}
