# Schema conventions

Every migration follows these. `migrations/0002_conventions.sql` provides the
machinery, `tests/schema.rs` proves nothing escaped it — including that no
table exists which never called `tezgah_register` — and `tests/isolation.rs`
puts a row in every registered table and asks a second scope to read, change
and delete it.

## Every table

```sql
create table product (
    id          uuid primary key,
    -- columns
    handle      text not null,
    ...
);
call tezgah_register('product');
```

`tezgah_register` adds `scope`, `created_at`, `updated_at`, an index on
`(scope, id)`, an `updated_at` trigger, forces row-level security, and writes
the name into `tezgah_table` so the schema test can find it.

- **Singular table names.** `product`, not `products`.
- **`id uuid primary key`** — no sequences, no composite primary keys. A
  UUIDv7 comes from `src/id.rs`.
- **`scope` first in every index.** A query is always within one scope, so an
  index that does not start there is the wrong index.
- **Uniqueness is per scope.** `unique (scope, handle)`, never `unique (handle)`.
- **Foreign keys are real**, including across domains, and every one is
  indexed. An unindexed foreign key turns a delete into a table scan.
- **Delete behaviour is chosen**, never left to the default: `on delete cascade`
  for something that cannot exist alone (a line item without its cart),
  `restrict` for something a person must deal with first (a product with
  orders), `set null` for a reference that may go away (a promotion on a
  historical order).
- **Money is two columns**: `amount numeric(20, 6)` beside
  `currency_code char(3)`. Never one without the other, never a float, never minor
  units.
- **Quantities are `integer` with `check (quantity > 0)`.** A zero line item is
  a deleted line item.
- **Enums are `text` with a check constraint**, not a Postgres enum: adding a
  value to a Postgres enum takes a lock and cannot be undone in a transaction.
  Name the constraint `<table>_<column>_valid`.
- **Timestamps are `timestamptz`.** There is no other kind.
- **Soft delete is `deleted_at timestamptz`**, and only where restoring is a
  feature. A partial index `where deleted_at is null` carries the live rows.

## Naming

- Foreign key column: the referenced table, singular, `_id`. `cart_id`.
- Where two point at one table, the role comes first: `shipping_address_id`,
  `billing_address_id`.
- Boolean columns read as an assertion: `is_default`, `allows_backorder`.
- Index: `<table>_<columns>_idx`. Unique constraint: `<table>_<columns>_key`.
- Check constraint: `<table>_<what>_valid`.

## Migration numbering

Each domain owns a number, so parallel work does not collide.

| File | Domain |
|---|---|
| `0001_scope` | the scope table and `tezgah_current_scope()` |
| `0002_conventions` | triggers, `tezgah_register`, the table registry |
| `0003_workflow` | the workflow runner's execution and step tables |
| `0004_store` | store, currency, region, sales channel, publishable key |
| `0005_catalogue` | product, variant, option, collection, category, tag, image |
| `0006_pricing` | price set, price, price rule, price list |
| `0007_inventory` | location, inventory item, level, reservation |
| `0008_customer` | customer, address, group |
| `0009_cart` | cart, line item, adjustment, tax line, shipping method |
| `0010_payment` | collection, session, payment, capture, refund, account holder |
| `0011_order` | order, item, change, change action, transaction ledger |
| `0012_fulfilment` | set, service zone, geo zone, shipping option, fulfilment |
| `0013_tax` | tax region, rate, rate rule |
| `0014_promotion` | promotion, application method, rules, campaign |
| `0018_session_cancelled_after_authorising` | lets a cancelled session keep when it authorised |
| `0015_payment_mismatch` | the `mismatch` collection status |
| `0016_pricing_link` | which price set answers for a variant and for a shipping option |
| `0017_workflow_parallel` | the group a workflow step runs in, for steps that run at once |

Migrations are append-only once merged. A change to a shipped table is a new
file, and it expands before it contracts: add and backfill in one release, read
from the new column in the next, drop the old one in a third.

## Long locks

Migrations set `lock_timeout` and `statement_timeout` at the top and create
indexes concurrently where the table may already be large. `alter table ... set
not null` scans the whole table: add the column nullable, backfill in batches,
add a `not null` check as `not valid`, then validate it.

## Rust side

- One module per domain under `src/`, named for the domain.
- Rows are structs deriving `sqlx::FromRow`, with typed ids from `src/id.rs`.
- Every public function takes `&mut Tx<'_>` and `&Ctx<'_>`, in that order.
- Nothing reads or writes without a `Permit` obtained from `ctx.permit(..)`.
- **Every scoped query names its scope**, `where scope = $1`, even though the
  policy already filters by it. The policy is the guarantee; the predicate is
  what still holds if a host connects as a table owner or a superuser, both of
  which bypass policies. It costs nothing — the index starts with `scope`.
- Audit rows, events and jobs go through `Ctx`, in the same transaction.
- Listing returns a `Page<T>` with a cursor. There is no unbounded list, and
  `tests/no_unbounded_list.rs` reads `src/` to say so: a public function
  returning a `Vec` must take a `Paging`, cap its query with a named `MAX_*`
  constant, or touch no database at all.
