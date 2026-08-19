import { region, salesChannel, type Region, type SalesChannel } from "@/api/views"
import { DataTable, type Columns } from "@/components/data-table"
import { Badge } from "@/components/ui/badge"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/screens/page-heading"

const regions: Columns<Region> = [
  { header: "Name", accessorKey: "name", meta: { className: "font-medium" } },
  {
    header: "Currency",
    accessorKey: "currency_code",
    meta: { className: "font-mono text-xs uppercase" },
  },
  {
    header: "Tax",
    accessorKey: "is_tax_inclusive",
    cell: ({ row }) => (
      <div className="flex items-center gap-1.5">
        <Badge variant="outline">
          {row.original.is_tax_inclusive ? "inclusive" : "exclusive"}
        </Badge>
        {row.original.has_automatic_taxes ? <Badge>automatic</Badge> : null}
      </div>
    ),
  },
  {
    header: "Providers",
    accessorKey: "payment_providers",
    cell: ({ row }) =>
      row.original.payment_providers.length ? (
        <span className="font-mono text-xs">
          {row.original.payment_providers.join(", ")}
        </span>
      ) : (
        <span className="text-muted-foreground text-xs">none</span>
      ),
    meta: { className: "text-right" },
  },
]

const channels: Columns<SalesChannel> = [
  { header: "Name", accessorKey: "name", meta: { className: "font-medium" } },
  {
    header: "Description",
    accessorKey: "description",
    cell: ({ row }) =>
      row.original.description ?? <span className="text-muted-foreground">—</span>,
    meta: { className: "max-w-96 truncate text-sm" },
  },
  {
    header: "State",
    accessorKey: "is_disabled",
    cell: ({ row }) => (
      <Badge variant={row.original.is_disabled ? "outline" : "default"}>
        {row.original.is_disabled ? "disabled" : "selling"}
      </Badge>
    ),
    meta: { className: "text-right" },
  },
]

export function Store() {
  const regionList = usePagedList(["regions"], "/admin/regions", region)
  const channelList = usePagedList(
    ["sales-channels"],
    "/admin/sales-channels",
    salesChannel,
  )

  return (
    <div className="space-y-4">
      <PageHeading
        title="Store"
        subtitle="Where the shop sells, and through what."
      />
      <Tabs defaultValue="regions">
        <TabsList>
          <TabsTrigger value="regions">Regions</TabsTrigger>
          <TabsTrigger value="channels">Sales channels</TabsTrigger>
        </TabsList>
        <TabsContent value="regions" className="pt-3">
          <DataTable
            paged={regionList}
            columns={regions}
            empty={{
              title: "No regions",
              description: "A region decides currency and how tax is shown.",
            }}
          />
        </TabsContent>
        <TabsContent value="channels" className="pt-3">
          <DataTable
            paged={channelList}
            columns={channels}
            empty={{
              title: "No sales channels",
              description: "A channel decides which products a storefront can see.",
            }}
          />
        </TabsContent>
      </Tabs>
    </div>
  )
}
