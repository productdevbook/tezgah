set lock_timeout = '3s';
set statement_timeout = '60s';

-- Rewrites a foreign key without having to know the name Postgres gave it.
-- `p_scoped` is what 0026 uses; here every call leaves the key single-column
-- and only changes what a delete does.
create or replace procedure tezgah_fk(
    p_child text,
    p_col text,
    p_parent text,
    p_delete text,
    p_scoped boolean default false
)
language plpgsql as $$
declare
    child     oid := quote_ident(p_child)::regclass;
    name      text := p_child || '_' || p_col || '_fkey';
    doomed    text;
    clause    text;
    scope_att smallint;
    col_att   smallint;
begin
    for doomed in
        select con.conname
        from pg_constraint con
        join pg_attribute a on a.attrelid = con.conrelid and a.attname = p_col
        where con.contype = 'f' and con.conrelid = child and a.attnum = any (con.conkey)
    loop
        execute format('alter table %I drop constraint %I', p_child, doomed);
    end loop;

    if p_scoped then
        -- A composite key's `set null` would null `scope` too, and `scope` is
        -- not null. Postgres 15 and later can name the column to clear.
        clause := case
            when p_delete = 'set null' then format('set null (%I)', p_col)
            else p_delete
        end;

        execute format(
            'alter table %I add constraint %I foreign key (scope, %I) '
            'references %I (scope, id) on delete %s not valid',
            p_child, name, p_col, p_parent, clause
        );
        execute format('alter table %I validate constraint %I', p_child, name);

        select attnum into scope_att from pg_attribute
        where attrelid = child and attname = 'scope';
        select attnum into col_att from pg_attribute
        where attrelid = child and attname = p_col;

        if not exists (
            select 1 from pg_index i
            where i.indrelid = child
              and array[scope_att, col_att]::smallint[] <@ (i.indkey::smallint[])
        ) then
            execute format('create index %I on %I (scope, %I)',
                           p_child || '_' || p_col || '_scope_idx', p_child, p_col);
        end if;
    else
        execute format(
            'alter table %I add constraint %I foreign key (%I) '
            'references %I (id) on delete %s',
            p_child, name, p_col, p_parent, p_delete
        );
    end if;
end
$$;

-- `cascade` is for something that cannot exist alone. A record of what
-- happened is not that: it is the one thing that cannot be reconstructed once
-- the row it hangs off is gone. Each of these exists to be evidence, so each
-- becomes `restrict` and a deleter has to say what it means to destroy.

-- Where an order has been, and who it was handed to.
call tezgah_fk('order_status_history', 'order_id', 'order', 'restrict');
call tezgah_fk('order_transfer', 'order_id', 'order', 'restrict');

-- What a promotion took off a line and what tax was charged on it. 0021 added
-- these precisely so the money was provable rather than reconstructed.
call tezgah_fk('order_line_item_adjustment', 'order_line_item_id', 'order_line_item', 'restrict');
call tezgah_fk('order_line_item_tax_line', 'order_line_item_id', 'order_line_item', 'restrict');
call tezgah_fk('order_shipping_method_adjustment', 'order_shipping_method_id',
               'order_shipping_method', 'restrict');
call tezgah_fk('order_shipping_method_tax_line', 'order_shipping_method_id',
               'order_shipping_method', 'restrict');

-- Redemption and spend ledgers. `promotion_usage.used` and `campaign_budget.used`
-- are counters nothing else can recount.
call tezgah_fk('promotion_usage', 'promotion_id', 'promotion', 'restrict');
call tezgah_fk('campaign_budget', 'campaign_id', 'campaign', 'restrict');

-- Amounts a session authorised, for sessions that never became a `payment`.
call tezgah_fk('payment_session', 'payment_collection_id', 'payment_collection', 'restrict');

-- What was in the box and what the tracking number was.
call tezgah_fk('fulfillment_item', 'fulfillment_id', 'fulfillment', 'restrict');
call tezgah_fk('fulfillment_label', 'fulfillment_id', 'fulfillment', 'restrict');

-- Left cascading on purpose: `fulfillment_set` → `service_zone` → `geo_zone`
-- and `shipping_option`, and a promotion's rule tables. Those are configuration
-- a shop edits, not a record of a thing that happened, and `fulfilment.rs`
-- deletes a set expecting its zones to go with it.

-- The list the schema test reads, so a table added tomorrow has to say which
-- side of that line it is on rather than defaulting to cascade unnoticed.
create table if not exists tezgah_evidence_table (
    name text primary key references tezgah_table (name)
);

insert into tezgah_evidence_table (name) values
    ('order_status_history'),
    ('order_transfer'),
    ('order_line_item_adjustment'),
    ('order_line_item_tax_line'),
    ('order_shipping_method_adjustment'),
    ('order_shipping_method_tax_line'),
    ('promotion_usage'),
    ('campaign_budget'),
    ('payment_session'),
    ('fulfillment_item'),
    ('fulfillment_label'),
    ('order_transaction'),
    ('order_summary')
on conflict do nothing;
