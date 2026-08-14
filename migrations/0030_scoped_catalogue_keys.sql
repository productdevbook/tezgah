set lock_timeout = '3s';
set statement_timeout = '120s';

-- 0026 drew the line at the order and money spine. This is the rest of it:
-- catalogue, pricing, inventory and cart. Postgres checks a foreign key with
-- row security bypassed, so one shop's cart line could reference another's
-- variant and one shop's price hang off another's price set — invisible to
-- every read and entirely real to the constraint.

-- `product_variant_option_value` is handled below rather than here: it carries
-- a genuine composite key that `tezgah_fk` would drop along with the
-- single-column one on the same column.
do $$
declare
    k record;
begin
    for k in
        select c.relname::text as child,
               a.attname::text as col,
               p.relname::text as parent,
               case con.confdeltype
                   when 'c' then 'cascade'
                   when 'n' then 'set null'
                   when 'r' then 'restrict'
                   when 'd' then 'set default'
                   else 'no action'
               end as del
        from pg_constraint con
        join pg_class c on c.oid = con.conrelid
        join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
        join pg_class p on p.oid = con.confrelid
        join pg_attribute a on a.attrelid = con.conrelid and a.attnum = con.conkey[1]
        where con.contype = 'f'
          and cardinality(con.conkey) = 1
          and c.relname in (
              'product_type', 'product_collection', 'product_tag', 'product_category',
              'product', 'product_variant', 'product_option', 'product_option_value',
              'product_image', 'product_tag_link', 'product_category_link',
              'product_sales_channel', 'product_translation',
              'price_set', 'price_list', 'price_list_rule', 'price', 'price_rule',
              'price_preference', 'product_variant_price_set', 'shipping_option_price_set',
              'stock_location_address', 'stock_location', 'stock_location_sales_channel',
              'inventory_item', 'inventory_level', 'reservation_item',
              'variant_inventory_item',
              'cart_address', 'cart', 'cart_line_item', 'cart_line_item_adjustment',
              'cart_line_item_tax_line', 'cart_shipping_method',
              'cart_shipping_method_adjustment', 'cart_shipping_method_tax_line',
              'payment_collection'
          )
          and exists (select 1 from tezgah_table t where t.name = c.relname)
          and exists (select 1 from tezgah_table t where t.name = p.relname)
          and a.attname <> 'scope'
        order by 1, 2
    loop
        call tezgah_fk(k.child, k.col, k.parent, k.del, true);
    end loop;
end
$$;

-- The one real composite in the schema: the pivot's own `option_id` is checked
-- against the value's, so "Size twice on one variant" is a unique violation.
-- Widened to carry the scope rather than replaced.
create unique index if not exists product_option_value_scope_id_option_key
    on product_option_value (scope, id, option_id);

do $$
declare
    doomed text;
begin
    for doomed in
        select con.conname
        from pg_constraint con
        where con.contype = 'f'
          and con.conrelid = 'product_variant_option_value'::regclass
    loop
        execute format('alter table product_variant_option_value drop constraint %I', doomed);
    end loop;
end
$$;

alter table product_variant_option_value
    add constraint product_variant_option_value_variant_id_fkey
        foreign key (scope, variant_id) references product_variant (scope, id)
        on delete cascade,
    add constraint product_variant_option_value_option_id_fkey
        foreign key (scope, option_id) references product_option (scope, id)
        on delete cascade,
    add constraint product_variant_option_value_option_value_id_fkey
        foreign key (scope, option_value_id, option_id)
            references product_option_value (scope, id, option_id)
        on delete restrict;

create index if not exists product_variant_option_value_scope_value_option_idx
    on product_variant_option_value (scope, option_value_id, option_id);
drop index if exists product_variant_option_value_value_option_idx;
drop index if exists product_option_value_id_option_key;

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

-- Registering a table says every one of its keys carries the scope. Said here
-- rather than left to the schema test, so a gap stops the migration.
do $$
declare
    bare text;
begin
    select string_agg(c.relname || '.' || con.conname, ', ') into bare
    from tezgah_scoped_fk_table f
    join pg_class c on c.relname = f.name
    join pg_namespace n on n.oid = c.relnamespace and n.nspname = 'public'
    join pg_constraint con on con.conrelid = c.oid and con.contype = 'f'
    where not exists (
        select 1 from pg_attribute a
        where a.attrelid = con.conrelid
          and a.attname = 'scope'
          and a.attnum = any (con.conkey)
    );

    if bare is not null then
        raise exception 'these keys would still cross a scope: %', bare;
    end if;
end
$$;
