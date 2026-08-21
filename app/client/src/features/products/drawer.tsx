import { useNavigate } from "@tanstack/react-router"
import type { ReactNode } from "react"
import { useT } from "@/panel/i18n"

import { product, type Product } from "@/api/schemas"
import { RouteDrawer } from "@/components/modals/route-drawer"
import { QueryState } from "@/components/query-state"
import { useDetail } from "@/lib/detail"

/**
 * The shell every one of a product's section editors is inside.
 *
 * A record's page is a stack of sections, and a section that can be changed
 * has its own address and its own drawer — `/products/$id/organisation` is
 * one form, not a tab of a big one. That is what keeps a save small enough to
 * describe: an operator who changed the origin country did not also submit
 * the title.
 *
 * They all `PATCH /admin/products/{id}`, because that is the one write the
 * crate offers for a product. Each sends only its own fields, which is the
 * whole difference: `UpdateProduct`'s fields are `Option`, so a field left out
 * is left alone.
 */
export function ProductDrawer({
  id,
  title,
  description,
  children,
}: {
  id: string
  title: string
  description?: string
  children: (item: Product) => ReactNode
}) {
  const t = useT()
  const navigate = useNavigate()
  const result = useDetail(["products"], "/admin/products/{id}", product, id)

  return (
    <RouteDrawer
      onClose={() => void navigate({ to: "/products/$id", params: { id } })}
    >
      <RouteDrawer.Header title={title} description={description} />
      {result.data ? (
        children(result.data)
      ) : (
        <RouteDrawer.Body>
          <QueryState
            query={result}
            empty={{
              title: t("empty.product"),
              description: t("general.nothingToShow"),
            }}
          >
            {() => null}
          </QueryState>
        </RouteDrawer.Body>
      )}
    </RouteDrawer>
  )
}
