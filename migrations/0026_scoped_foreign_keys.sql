set lock_timeout = '3s';
set statement_timeout = '60s';

-- `SCHEMA.md` promises real foreign keys and forced row-level security, and
-- those two promises never met. Postgres validates a foreign key with RLS
-- bypassed — referential integrity runs as the system, not under the policy —
-- so a row in scope A could reference a row in scope B, invisibly to every
-- read and entirely real to the constraint, with `cascade` and `set null`
-- firing across the boundary.
--
-- A scoped key needs `(scope, id)` to be unique on the parent, and
-- `tezgah_scoped` only ever made it a plain index.

create or replace procedure tezgah_scoped(table_name text)
language plpgsql as $$
declare
    quoted text := quote_ident(table_name);
begin
    execute format('alter table %s add column if not exists scope uuid not null', quoted);
    execute format(
        'alter table %s add column if not exists created_at timestamptz not null default now()',
        quoted
    );
    execute format(
        'alter table %s add column if not exists updated_at timestamptz not null default now()',
        quoted
    );

    -- Unique rather than a plain index: `id` is already the primary key, so
    -- this costs nothing the index did not, and it is what lets a child say
    -- `references t (scope, id)`.
    if not exists (
        select 1 from pg_constraint
        where conrelid = quoted::regclass
          and conname = table_name || '_scope_key'
    ) then
        execute format('alter table %s add constraint %s unique (scope, id)',
                       quoted, quote_ident(table_name || '_scope_key'));
    end if;
    execute format('drop index if exists %s', quote_ident(table_name || '_scope_idx'));

    execute format('drop trigger if exists %s on %s',
        quote_ident(table_name || '_touch'), quoted);
    execute format(
        'create trigger %s before update on %s for each row execute function tezgah_touch()',
        quote_ident(table_name || '_touch'), quoted
    );

    execute format('alter table %s enable row level security', quoted);
    execute format('alter table %s force row level security', quoted);
    execute format('drop policy if exists %s on %s',
        quote_ident(table_name || '_scope'), quoted);
    execute format(
        'create policy %s on %s using (scope = tezgah_current_scope()) '
        'with check (scope = tezgah_current_scope())',
        quote_ident(table_name || '_scope'), quoted
    );
end
$$;

do $$
declare
    t text;
begin
    for t in select name from tezgah_table order by name loop
        if not exists (
            select 1 from pg_constraint
            where conrelid = quote_ident(t)::regclass and conname = t || '_scope_key'
        ) then
            execute format('alter table %I add constraint %I unique (scope, id)',
                           t, t || '_scope_key');
        end if;
        execute format('drop index if exists %I', t || '_scope_idx');
    end loop;
end
$$;

-- The order and the money that decides it. Rewriting every key in the schema
-- is a bigger change than this file wants to be, so the line is drawn where a
-- crossing costs money or another shop's order: the order spine, its returns,
-- exchanges and claims, the payment chain that settles it, and the per-line
-- adjustments and tax lines 0021 added to make that money provable. The
-- catalogue, pricing, inventory and cart keys stay single-column for now and
-- are guarded only by the `x.scope = y.scope` joins, which is where they were
-- yesterday.

call tezgah_fk('order', 'region_id', 'region', 'restrict', true);
call tezgah_fk('order', 'sales_channel_id', 'sales_channel', 'restrict', true);
call tezgah_fk('order', 'shipping_address_id', 'order_address', 'restrict', true);
call tezgah_fk('order', 'billing_address_id', 'order_address', 'restrict', true);
call tezgah_fk('order', 'payment_collection_id', 'payment_collection', 'restrict', true);

call tezgah_fk('order_line_item', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_line_item', 'variant_id', 'product_variant', 'set null', true);
call tezgah_fk('order_line_item', 'product_id', 'product', 'set null', true);

call tezgah_fk('order_item', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_item', 'order_line_item_id', 'order_line_item', 'restrict', true);

call tezgah_fk('order_shipping_method', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_summary', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_transaction', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_credit_line', 'order_id', 'order', 'restrict', true);

call tezgah_fk('order_return', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_return_item', 'order_return_id', 'order_return', 'restrict', true);
call tezgah_fk('order_return_item', 'order_line_item_id', 'order_line_item', 'restrict', true);
call tezgah_fk('order_return_item', 'return_reason_id', 'return_reason', 'set null', true);

call tezgah_fk('order_exchange', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_exchange', 'order_return_id', 'order_return', 'restrict', true);
call tezgah_fk('order_exchange_item', 'order_exchange_id', 'order_exchange', 'restrict', true);
call tezgah_fk('order_exchange_item', 'order_line_item_id', 'order_line_item', 'restrict', true);

call tezgah_fk('order_claim', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_claim', 'order_return_id', 'order_return', 'restrict', true);
call tezgah_fk('order_claim_item', 'order_claim_id', 'order_claim', 'restrict', true);
call tezgah_fk('order_claim_item', 'order_line_item_id', 'order_line_item', 'restrict', true);

call tezgah_fk('order_change', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_change', 'order_return_id', 'order_return', 'restrict', true);
call tezgah_fk('order_change', 'order_exchange_id', 'order_exchange', 'restrict', true);
call tezgah_fk('order_change', 'order_claim_id', 'order_claim', 'restrict', true);

call tezgah_fk('order_change_action', 'order_change_id', 'order_change', 'restrict', true);
call tezgah_fk('order_change_action', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_change_action', 'order_return_id', 'order_return', 'restrict', true);
call tezgah_fk('order_change_action', 'order_exchange_id', 'order_exchange', 'restrict', true);
call tezgah_fk('order_change_action', 'order_claim_id', 'order_claim', 'restrict', true);

call tezgah_fk('order_status_history', 'order_id', 'order', 'restrict', true);
call tezgah_fk('order_transfer', 'order_id', 'order', 'restrict', true);

call tezgah_fk('order_line_item_adjustment', 'order_line_item_id',
               'order_line_item', 'restrict', true);
call tezgah_fk('order_line_item_tax_line', 'order_line_item_id',
               'order_line_item', 'restrict', true);
call tezgah_fk('order_shipping_method_adjustment', 'order_shipping_method_id',
               'order_shipping_method', 'restrict', true);
call tezgah_fk('order_shipping_method_tax_line', 'order_shipping_method_id',
               'order_shipping_method', 'restrict', true);

call tezgah_fk('payment_session', 'payment_collection_id',
               'payment_collection', 'restrict', true);
call tezgah_fk('payment', 'payment_collection_id', 'payment_collection', 'restrict', true);
call tezgah_fk('payment', 'payment_session_id', 'payment_session', 'restrict', true);
call tezgah_fk('capture', 'payment_id', 'payment', 'restrict', true);
call tezgah_fk('refund', 'payment_id', 'payment', 'restrict', true);

-- The three columns that opted out of foreign keys altogether. They are added
-- `not valid`: the key never existed, so a row pointing at nothing is legal
-- history rather than corruption, and validating would abort on data the
-- schema itself allowed. New and updated rows are held to it from here.
create index if not exists order_transfer_from_customer_idx
    on order_transfer (scope, from_customer_id);

alter table fulfillment_item
    add constraint fulfillment_item_line_item_id_fkey
        foreign key (scope, line_item_id) references order_line_item (scope, id)
        on delete restrict not valid;

alter table promotion_usage
    add constraint promotion_usage_customer_id_fkey
        foreign key (scope, customer_id) references customer (scope, id)
        on delete restrict not valid;

alter table order_transfer
    add constraint order_transfer_from_customer_id_fkey
        foreign key (scope, from_customer_id) references customer (scope, id)
        on delete set null (from_customer_id) not valid;

alter table order_transfer
    add constraint order_transfer_to_customer_id_fkey
        foreign key (scope, to_customer_id) references customer (scope, id)
        on delete set null (to_customer_id) not valid;

-- The tables whose keys must stay scoped, so the schema test asks the
-- catalogue rather than a list somebody maintains by hand.
create table if not exists tezgah_scoped_fk_table (
    name text primary key references tezgah_table (name)
);

insert into tezgah_scoped_fk_table (name)
select distinct c.relname::text
from pg_constraint con
join pg_class c on c.oid = con.conrelid
join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
join pg_attribute a on a.attrelid = con.conrelid and a.attname = 'scope'
where con.contype = 'f'
  and a.attnum = any (con.conkey)
  and exists (select 1 from tezgah_table t where t.name = c.relname)
on conflict do nothing;
