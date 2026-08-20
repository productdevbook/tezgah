import { useState, type FormEvent } from "react"
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { HugeiconsIcon } from "@hugeicons/react"
import { Add01Icon, Copy01Icon, Tick02Icon } from "@hugeicons/core-free-icons"
import { z } from "zod"

import { get, post } from "@/api/client"
import {
  createCurrency,
  createPublishableKey,
  createRegion,
  createSalesChannel,
  currency,
  issuedKey,
  region,
  salesChannel,
  type Currency,
  type CreateCurrency,
  type CreatePublishableKey,
  type CreateRegion,
  type CreateSalesChannel,
  type IssuedKey,
  type Region,
  type SalesChannel,
} from "@/api/views"
import { DataTable, type Columns } from "@/components/data-table"
import { FormError } from "@/components/form-error"
import { QueryState } from "@/components/query-state"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog"
import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Switch } from "@/components/ui/switch"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { usePagedList } from "@/lib/paged"
import { PageHeading } from "@/screens/page-heading"

const regionColumns: Columns<Region> = [
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
        <span className="text-xs text-muted-foreground">none</span>
      ),
    meta: { className: "text-right" },
  },
]

const channelColumns: Columns<SalesChannel> = [
  { header: "Name", accessorKey: "name", meta: { className: "font-medium" } },
  {
    header: "Description",
    accessorKey: "description",
    cell: ({ row }) =>
      row.original.description ?? (
        <span className="text-muted-foreground">—</span>
      ),
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
    salesChannel
  )
  const currencyList = useQuery({
    queryKey: ["currencies"],
    queryFn: ({ signal }) =>
      get("/admin/currencies", { signal, schema: z.array(currency) }),
  })

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
          <TabsTrigger value="currencies">Currencies</TabsTrigger>
          <TabsTrigger value="keys">Publishable keys</TabsTrigger>
        </TabsList>
        <TabsContent value="regions" className="space-y-3 pt-3">
          <div className="flex justify-end">
            <NewRegionDialog />
          </div>
          <DataTable
            paged={regionList}
            columns={regionColumns}
            empty={{
              title: "No regions",
              description: "A region decides currency and how tax is shown.",
            }}
          />
        </TabsContent>
        <TabsContent value="channels" className="space-y-3 pt-3">
          <div className="flex justify-end">
            <NewSalesChannelDialog />
          </div>
          <DataTable
            paged={channelList}
            columns={channelColumns}
            empty={{
              title: "No sales channels",
              description:
                "A channel decides which products a storefront can see.",
            }}
          />
        </TabsContent>
        <TabsContent value="currencies" className="space-y-3 pt-3">
          <div className="flex justify-end">
            <NewCurrencyDialog />
          </div>
          <CurrencyList query={currencyList} />
        </TabsContent>
        <TabsContent value="keys" className="space-y-3 pt-3">
          <PublishableKeyPanel />
        </TabsContent>
      </Tabs>
    </div>
  )
}

/**
 * `GET /admin/currencies` answers a plain array, not `Page<T>`
 * (`server/src/http/admin.rs`'s `list_currencies`) — `DataTable` wants a
 * cursor, so this is its own small table rather than a `usePagedList` call.
 */
function CurrencyList({
  query,
}: {
  query: ReturnType<typeof useQuery<Currency[]>>
}) {
  return (
    <QueryState
      query={query}
      empty={{
        title: "No currencies",
        description: "Nothing prices or opens a cart until a shop enables one.",
      }}
    >
      {(currencies) => (
        <div className="rounded-md border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Code</TableHead>
                <TableHead>Name</TableHead>
                <TableHead>Symbol</TableHead>
                <TableHead className="text-right">Exponent</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {currencies.map((c) => (
                <TableRow key={c.code}>
                  <TableCell className="font-mono text-xs uppercase">
                    {c.code}
                  </TableCell>
                  <TableCell className="font-medium">{c.name}</TableCell>
                  <TableCell>{c.symbol}</TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {c.exponent}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
    </QueryState>
  )
}

const EMPTY_CURRENCY_FORM = {
  code: "",
  numeric_code: "",
  exponent: "2",
  symbol: "",
  symbol_native: "",
  name: "",
}

function NewCurrencyDialog() {
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY_CURRENCY_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateCurrency) =>
      post("/admin/currencies", { schema: currency, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["currencies"] })
      setOpen(false)
      setForm(EMPTY_CURRENCY_FORM)
      setFieldErrors({})
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createCurrency.safeParse({
      code: form.code,
      numeric_code:
        form.numeric_code.trim() === "" ? undefined : form.numeric_code,
      exponent: Number(form.exponent),
      symbol: form.symbol,
      symbol_native: form.symbol_native,
      name: form.name,
    })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setForm(EMPTY_CURRENCY_FORM)
          setFieldErrors({})
          mutation.reset()
        }
      }}
    >
      <DialogTrigger render={<Button size="sm" variant="outline" />}>
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        New currency
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Enable a currency</DialogTitle>
          <DialogDescription>
            tezgah keeps no built-in list. Enabling one twice corrects it rather
            than conflicting.
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={submit}>
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <div className="grid grid-cols-2 gap-4">
            <Field data-invalid={!!fieldErrors.code}>
              <FieldLabel htmlFor="currency-code">Code</FieldLabel>
              <Input
                id="currency-code"
                value={form.code}
                onChange={(e) => setForm({ ...form, code: e.target.value })}
                placeholder="USD"
                maxLength={3}
                aria-invalid={!!fieldErrors.code}
              />
              <FieldError>{fieldErrors.code}</FieldError>
            </Field>
            <Field data-invalid={!!fieldErrors.exponent}>
              <FieldLabel htmlFor="currency-exponent">Exponent</FieldLabel>
              <Input
                id="currency-exponent"
                type="number"
                min={0}
                max={4}
                value={form.exponent}
                onChange={(e) => setForm({ ...form, exponent: e.target.value })}
                aria-invalid={!!fieldErrors.exponent}
              />
              <FieldError>{fieldErrors.exponent}</FieldError>
            </Field>
          </div>
          <Field data-invalid={!!fieldErrors.name}>
            <FieldLabel htmlFor="currency-name">Name</FieldLabel>
            <Input
              id="currency-name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="US Dollar"
              aria-invalid={!!fieldErrors.name}
            />
            <FieldError>{fieldErrors.name}</FieldError>
          </Field>
          <div className="grid grid-cols-2 gap-4">
            <Field data-invalid={!!fieldErrors.symbol}>
              <FieldLabel htmlFor="currency-symbol">Symbol</FieldLabel>
              <Input
                id="currency-symbol"
                value={form.symbol}
                onChange={(e) => setForm({ ...form, symbol: e.target.value })}
                placeholder="$"
                aria-invalid={!!fieldErrors.symbol}
              />
              <FieldError>{fieldErrors.symbol}</FieldError>
            </Field>
            <Field data-invalid={!!fieldErrors.symbol_native}>
              <FieldLabel htmlFor="currency-symbol-native">
                Native symbol
              </FieldLabel>
              <Input
                id="currency-symbol-native"
                value={form.symbol_native}
                onChange={(e) =>
                  setForm({ ...form, symbol_native: e.target.value })
                }
                placeholder="$"
                aria-invalid={!!fieldErrors.symbol_native}
              />
              <FieldError>{fieldErrors.symbol_native}</FieldError>
            </Field>
          </div>
          <Field>
            <FieldLabel htmlFor="currency-numeric">
              Numeric code (optional)
            </FieldLabel>
            <Input
              id="currency-numeric"
              value={form.numeric_code}
              onChange={(e) =>
                setForm({ ...form, numeric_code: e.target.value })
              }
              placeholder="840"
              maxLength={3}
            />
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Enabling…" : "Enable currency"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

const EMPTY_REGION_FORM = {
  name: "",
  currency_code: "",
  is_tax_inclusive: false,
  has_automatic_taxes: true,
}

function NewRegionDialog() {
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY_REGION_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateRegion) =>
      post("/admin/regions", { schema: region, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["regions"] })
      setOpen(false)
      setForm(EMPTY_REGION_FORM)
      setFieldErrors({})
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createRegion.safeParse(form)
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setForm(EMPTY_REGION_FORM)
          setFieldErrors({})
          mutation.reset()
        }
      }}
    >
      <DialogTrigger render={<Button size="sm" />}>
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        New region
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New region</DialogTitle>
          <DialogDescription>
            A region decides currency and how tax is shown.
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={submit}>
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <Field data-invalid={!!fieldErrors.name}>
            <FieldLabel htmlFor="region-name">Name</FieldLabel>
            <Input
              id="region-name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Europe"
              aria-invalid={!!fieldErrors.name}
            />
            <FieldError>{fieldErrors.name}</FieldError>
          </Field>
          <Field data-invalid={!!fieldErrors.currency_code}>
            <FieldLabel htmlFor="region-currency">Currency code</FieldLabel>
            <Input
              id="region-currency"
              value={form.currency_code}
              onChange={(e) =>
                setForm({ ...form, currency_code: e.target.value })
              }
              placeholder="EUR"
              maxLength={3}
              aria-invalid={!!fieldErrors.currency_code}
            />
            <FieldError>{fieldErrors.currency_code}</FieldError>
          </Field>
          <Field orientation="horizontal">
            <Switch
              id="region-tax-inclusive"
              checked={form.is_tax_inclusive}
              onCheckedChange={(checked) =>
                setForm({ ...form, is_tax_inclusive: checked })
              }
            />
            <FieldLabel htmlFor="region-tax-inclusive">
              Prices include tax
            </FieldLabel>
          </Field>
          <Field orientation="horizontal">
            <Switch
              id="region-automatic-taxes"
              checked={form.has_automatic_taxes}
              onCheckedChange={(checked) =>
                setForm({ ...form, has_automatic_taxes: checked })
              }
            />
            <FieldLabel htmlFor="region-automatic-taxes">
              Automatic taxes
            </FieldLabel>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Creating…" : "Create region"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

const EMPTY_CHANNEL_FORM = { name: "", description: "", is_disabled: false }

function NewSalesChannelDialog() {
  const client = useQueryClient()
  const [open, setOpen] = useState(false)
  const [form, setForm] = useState(EMPTY_CHANNEL_FORM)
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})

  const mutation = useMutation({
    mutationFn: (body: CreateSalesChannel) =>
      post("/admin/sales-channels", { schema: salesChannel, body }),
    onSuccess: () => {
      void client.invalidateQueries({ queryKey: ["sales-channels"] })
      setOpen(false)
      setForm(EMPTY_CHANNEL_FORM)
      setFieldErrors({})
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createSalesChannel.safeParse({
      name: form.name,
      description:
        form.description.trim() === "" ? undefined : form.description,
      is_disabled: form.is_disabled,
    })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) {
          setForm(EMPTY_CHANNEL_FORM)
          setFieldErrors({})
          mutation.reset()
        }
      }}
    >
      <DialogTrigger render={<Button size="sm" />}>
        <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
        New sales channel
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>New sales channel</DialogTitle>
          <DialogDescription>
            A channel decides which products a storefront can see.
          </DialogDescription>
        </DialogHeader>
        <form className="space-y-4" onSubmit={submit}>
          {mutation.isError ? <FormError error={mutation.error} /> : null}
          <Field data-invalid={!!fieldErrors.name}>
            <FieldLabel htmlFor="channel-name">Name</FieldLabel>
            <Input
              id="channel-name"
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="Web storefront"
              aria-invalid={!!fieldErrors.name}
            />
            <FieldError>{fieldErrors.name}</FieldError>
          </Field>
          <Field>
            <FieldLabel htmlFor="channel-description">Description</FieldLabel>
            <Input
              id="channel-description"
              value={form.description}
              onChange={(e) =>
                setForm({ ...form, description: e.target.value })
              }
            />
          </Field>
          <Field orientation="horizontal">
            <Switch
              id="channel-disabled"
              checked={form.is_disabled}
              onCheckedChange={(checked) =>
                setForm({ ...form, is_disabled: checked })
              }
            />
            <FieldLabel htmlFor="channel-disabled">Start disabled</FieldLabel>
          </Field>
          <DialogFooter>
            <Button type="submit" disabled={mutation.isPending}>
              {mutation.isPending ? "Creating…" : "Create sales channel"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/**
 * `GET /admin/publishable-api-keys` is not bound (`server/README.md`'s route
 * table), so there is nothing here to list — only to mint. The raw token
 * comes back once (`store::create_publishable_key`'s own doc comment) and is
 * not stored anywhere it could be read back, so this panel does not pretend
 * otherwise either: it holds the token in memory for this render only.
 */
function PublishableKeyPanel() {
  const [open, setOpen] = useState(false)
  const [title, setTitle] = useState("")
  const [fieldErrors, setFieldErrors] = useState<Record<string, string>>({})
  const [issued, setIssued] = useState<IssuedKey | null>(null)

  const mutation = useMutation({
    mutationFn: (body: CreatePublishableKey) =>
      post("/admin/publishable-api-keys", { schema: issuedKey, body }),
    onSuccess: (key) => {
      setIssued(key)
      setOpen(false)
      setTitle("")
      setFieldErrors({})
    },
  })

  function submit(event: FormEvent) {
    event.preventDefault()
    const parsed = createPublishableKey.safeParse({ title })
    if (!parsed.success) {
      const errors: Record<string, string> = {}
      for (const issue of parsed.error.issues)
        errors[String(issue.path[0])] = issue.message
      setFieldErrors(errors)
      return
    }
    setFieldErrors({})
    mutation.mutate(parsed.data)
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between gap-3 rounded-md border px-4 py-3">
        <div>
          <p className="text-sm font-medium">Mint a publishable key</p>
          <p className="text-xs text-muted-foreground">
            What a storefront sends as <code>x-publishable-key</code>. Shown
            once.
          </p>
        </div>
        <Dialog
          open={open}
          onOpenChange={(next) => {
            setOpen(next)
            if (!next) {
              setTitle("")
              setFieldErrors({})
              mutation.reset()
            }
          }}
        >
          <DialogTrigger render={<Button size="sm" />}>
            <HugeiconsIcon icon={Add01Icon} strokeWidth={2} />
            Mint key
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Mint a publishable key</DialogTitle>
              <DialogDescription>
                The token is shown once, right after this. Losing it means
                minting another.
              </DialogDescription>
            </DialogHeader>
            <form className="space-y-4" onSubmit={submit}>
              {mutation.isError ? <FormError error={mutation.error} /> : null}
              <Field data-invalid={!!fieldErrors.title}>
                <FieldLabel htmlFor="key-title">Title</FieldLabel>
                <Input
                  id="key-title"
                  value={title}
                  onChange={(e) => setTitle(e.target.value)}
                  placeholder="Storefront"
                  aria-invalid={!!fieldErrors.title}
                />
                <FieldError>{fieldErrors.title}</FieldError>
              </Field>
              <DialogFooter>
                <Button type="submit" disabled={mutation.isPending}>
                  {mutation.isPending ? "Minting…" : "Mint key"}
                </Button>
              </DialogFooter>
            </form>
          </DialogContent>
        </Dialog>
      </div>
      {issued ? <IssuedKeyCard issued={issued} /> : null}
    </div>
  )
}

function IssuedKeyCard({ issued }: { issued: IssuedKey }) {
  const [copied, setCopied] = useState(false)

  return (
    <div className="space-y-2 rounded-md border border-primary/30 bg-primary/5 px-4 py-3">
      <p className="text-sm font-medium">{issued.title}</p>
      <p className="text-xs text-muted-foreground">
        This will not be shown again — copy it now.
      </p>
      <div className="flex items-center gap-2">
        <code className="min-w-0 flex-1 truncate rounded bg-muted px-2 py-1.5 text-xs">
          {issued.token}
        </code>
        <Button
          type="button"
          size="icon-sm"
          variant="outline"
          onClick={() => {
            void navigator.clipboard.writeText(issued.token).then(() => {
              setCopied(true)
              setTimeout(() => setCopied(false), 2000)
            })
          }}
        >
          <HugeiconsIcon
            icon={copied ? Tick02Icon : Copy01Icon}
            strokeWidth={2}
          />
          <span className="sr-only">Copy</span>
        </Button>
      </div>
    </div>
  )
}
